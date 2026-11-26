import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { AgentBanner } from '@protocol/agentSetup'
import type { Config, ConfigFile } from '@protocol/config'
import { CHANNEL, type PtySize, type SessionId, type TacoShellsApi } from '@protocol/ipc'
import type { TerminalEvent, TerminalRequest } from '@protocol/runtimeMessages'
import type { DrawnState, WorkspaceRequest } from '@protocol/workspace'

const eventListeners = new Set<(event: TerminalEvent) => void>()
const queuedRequests: TerminalRequest[] = []
let runtimePort: MessagePort | null = null

function sendRequest(request: TerminalRequest): void {
  if (runtimePort === null) {
    queuedRequests.push(request)
    return
  }
  runtimePort.postMessage(request)
}

function listenOn(port: MessagePort): void {
  port.onmessage = (message) => {
    const event: TerminalEvent = message.data
    for (const listener of eventListeners) {
      listener(event)
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
  for (const request of queuedRequests) {
    port.postMessage(request)
  }
  queuedRequests.length = 0
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
      sendRequest({ type: 'write', id, data })
    },
    resize: (id: SessionId, size: PtySize) => {
      sendRequest({ type: 'resize', id, size })
    },
    attach: (id: SessionId, size: PtySize | null, view: number) => {
      sendRequest({ type: 'attach', id, size, view })
    },
    detach: (id: SessionId) => {
      sendRequest({ type: 'detach', id })
    },
    clear: (id: SessionId) => {
      sendRequest({ type: 'clear', id })
    },
    onEvent: (listener: (event: TerminalEvent) => void) => addListener(eventListeners, listener)
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
