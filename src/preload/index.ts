import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { Config, ConfigFile } from '@shared/config'
import {
  CHANNEL,
  type ClaudeSignalEvent,
  type PtyCreateOptions,
  type PtyDataEvent,
  type PtyExitEvent,
  type PtySize,
  type SessionId,
  type TacoShellsApi
} from '@shared/ipc'
import type { Workspace } from '@shared/model'
import type { Snapshot } from '@shared/snapshot'

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
    onFocusSession: (listener: (id: SessionId) => void) => subscribe(CHANNEL.focusSession, listener),
    onAttention: (listener: (id: SessionId) => void) => subscribe(CHANNEL.attentionSession, listener),
    onClaudeSignal: (listener: (event: ClaudeSignalEvent) => void) => subscribe(CHANNEL.claudeSignal, listener),
    onActivated: (listener: () => void) => subscribe(CHANNEL.appActivated, () => listener()),
    onUnavailable: (listener: () => void) => subscribe(CHANNEL.focusUnavailable, () => listener())
  },
  fullScreen: {
    initial: ipcRenderer.sendSync(CHANNEL.fullScreenGet) as boolean,
    onChange: (listener: (fullScreen: boolean) => void) => subscribe(CHANNEL.fullScreenChanged, listener)
  },
  snapshot: {
    initial: ipcRenderer.sendSync(CHANNEL.snapshotGet) as Snapshot | null,
    workspaceChanged: (workspace: Workspace) => ipcRenderer.send(CHANNEL.snapshotWorkspace, workspace)
  },
  update: {
    initial: ipcRenderer.sendSync(CHANNEL.updateGet) as string | null,
    onReady: (listener: (version: string) => void) => subscribe(CHANNEL.updateReady, listener),
    onProblem: (listener: (message: string) => void) => subscribe(CHANNEL.updateProblem, listener),
    apply: () => ipcRenderer.send(CHANNEL.updateApply)
  }
}

contextBridge.exposeInMainWorld('tacoShells', api)
