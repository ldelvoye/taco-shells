import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { AgentBanner } from '@protocol/agentSetup'
import type { Config, ConfigFile } from '@protocol/config'
import {
  CHANNEL,
  type PtyDataEvent,
  type PtyExitEvent,
  type PtySize,
  type SessionId,
  type TacoShellsApi
} from '@protocol/ipc'
import type { TerminalInput, TerminalOutput } from '@protocol/runtimeMessages'
import type { DrawnState, WorkspaceRequest } from '@protocol/workspace'

const dataListeners = new Set<(event: PtyDataEvent) => void>()
const exitListeners = new Set<(event: PtyExitEvent) => void>()
const queuedInput: TerminalInput[] = []
let runtimePort: MessagePort | null = null

function sendInput(input: TerminalInput): void {
  if (runtimePort === null) {
    queuedInput.push(input)
    return
  }
  runtimePort.postMessage(input)
}

function listenOn(port: MessagePort): void {
  port.onmessage = (message) => {
    const output: TerminalOutput = message.data
    if (output.type === 'data') {
      for (const listener of dataListeners) {
        listener(output.event)
      }
      return
    }
    for (const listener of exitListeners) {
      listener(output.event)
    }
  }
}

function addListener<T>(listeners: Set<(event: T) => void>, listener: (event: T) => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

ipcRenderer.on(CHANNEL.runtimePort, (event) => {
  const port = event.ports[0]
  runtimePort = port
  listenOn(port)
  for (const input of queuedInput) {
    port.postMessage(input)
  }
  queuedInput.length = 0
})
ipcRenderer.send(CHANNEL.runtimeConnect)

function subscribe<T>(channel: string, listener: (payload: T) => void): () => void {
  const forward = (_event: IpcRendererEvent, payload: T): void => {
    listener(payload)
  }
  ipcRenderer.on(channel, forward)
  return () => {
    ipcRenderer.off(channel, forward)
  }
}

const api: TacoShellsApi = {
  pty: {
    write: (id: SessionId, data: string) => {
      sendInput({ type: 'write', id, data })
    },
    resize: (id: SessionId, size: PtySize) => {
      sendInput({ type: 'resize', id, size })
    },
    listening: () => {
      sendInput({ type: 'listening' })
    },
    onData: (listener: (event: PtyDataEvent) => void) => addListener(dataListeners, listener),
    onExit: (listener: (event: PtyExitEvent) => void) => addListener(exitListeners, listener)
  },
  clipboard: {
    read: () => ipcRenderer.invoke(CHANNEL.clipboardRead),
    write: (text: string) => ipcRenderer.send(CHANNEL.clipboardWrite, text)
  },
  link: {
    open: (url: string) => ipcRenderer.send(CHANNEL.linkOpen, url)
  },
  config: {
    // Synchronous on purpose: the renderer sets the palette and builds its first
    // terminal from this, and an await here would paint the wrong theme first.
    initial: ipcRenderer.sendSync(CHANNEL.configGet) as Config,
    onChange: (listener: (config: Config) => void) => subscribe(CHANNEL.configChanged, listener),
    pathOf: (which: ConfigFile) => ipcRenderer.sendSync(CHANNEL.configPath, which) as string
  },
  focus: {
    onUnavailable: (listener: () => void) => subscribe(CHANNEL.focusUnavailable, () => listener())
  },
  fullScreen: {
    initial: ipcRenderer.sendSync(CHANNEL.fullScreenGet) as boolean,
    onChange: (listener: (fullScreen: boolean) => void) => subscribe(CHANNEL.fullScreenChanged, listener)
  },
  workspace: {
    initial: ipcRenderer.sendSync(CHANNEL.workspaceGet) as DrawnState,
    request: (request: WorkspaceRequest) => ipcRenderer.invoke(CHANNEL.workspaceRequest, request),
    onState: (listener: (state: DrawnState) => void) => subscribe(CHANNEL.workspaceState, listener)
  },
  update: {
    initial: ipcRenderer.sendSync(CHANNEL.updateGet) as string | null,
    onReady: (listener: (version: string) => void) => subscribe(CHANNEL.updateReady, listener),
    onProblem: (listener: (message: string) => void) => subscribe(CHANNEL.updateProblem, listener),
    apply: () => ipcRenderer.send(CHANNEL.updateApply)
  },
  agents: {
    initial: ipcRenderer.sendSync(CHANNEL.agentsGet) as AgentBanner[],
    onChange: (listener: (banners: AgentBanner[]) => void) => subscribe(CHANNEL.agentsChanged, listener),
    setUp: (harness: string) => ipcRenderer.send(CHANNEL.agentsSetUp, harness),
    decline: (harness: string) => ipcRenderer.send(CHANNEL.agentsDecline, harness),
    dismiss: (harness: string) => ipcRenderer.send(CHANNEL.agentsDismiss, harness)
  }
}

contextBridge.exposeInMainWorld('tacoShells', api)
