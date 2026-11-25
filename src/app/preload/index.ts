import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { AgentBanner } from '@protocol/agentSetup'
import type { Config, ConfigFile } from '@protocol/config'
import {
  CHANNEL,
  type PtyCreateOptions,
  type PtyDataEvent,
  type PtyExitEvent,
  type PtySize,
  type SessionId,
  type TacoShellsApi
} from '@protocol/ipc'
import type { Snapshot } from '@protocol/snapshot'
import type { DrawnState, WorkspaceRequest } from '@protocol/workspace'

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
    create: (size: PtySize, options?: PtyCreateOptions) => ipcRenderer.invoke(CHANNEL.ptyCreate, size, options),
    write: (id: SessionId, data: string) => ipcRenderer.send(CHANNEL.ptyWrite, id, data),
    resize: (id: SessionId, size: PtySize) => ipcRenderer.send(CHANNEL.ptyResize, id, size),
    kill: (id: SessionId) => ipcRenderer.send(CHANNEL.ptyKill, id),
    onData: (listener: (event: PtyDataEvent) => void) => subscribe(CHANNEL.ptyData, listener),
    onExit: (listener: (event: PtyExitEvent) => void) => subscribe(CHANNEL.ptyExit, listener)
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
  snapshot: {
    initial: ipcRenderer.sendSync(CHANNEL.snapshotGet) as Snapshot | null
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
