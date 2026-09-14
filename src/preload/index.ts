import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import {
  CHANNEL,
  type PtyDataEvent,
  type PtyExitEvent,
  type PtySize,
  type SessionId,
  type TaqueriaApi
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

const api: TaqueriaApi = {
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
  }
}

contextBridge.exposeInMainWorld('taqueria', api)
