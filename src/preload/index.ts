import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { Config, ConfigFile } from '@shared/config'
import {
  CHANNEL,
  type PtyDataEvent,
  type PtyExitEvent,
  type PtySize,
  type SessionId,
  type TacoShellsApi
} from '@shared/ipc'

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
    create: (size: PtySize, cwdFrom?: SessionId) => ipcRenderer.invoke(CHANNEL.ptyCreate, size, cwdFrom),
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
    onActivated: (listener: () => void) => subscribe(CHANNEL.appActivated, () => listener()),
    onUnavailable: (listener: () => void) => subscribe(CHANNEL.focusUnavailable, () => listener())
  }
}

contextBridge.exposeInMainWorld('tacoShells', api)
