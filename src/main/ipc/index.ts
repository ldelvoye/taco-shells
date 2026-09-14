import { type BrowserWindow, clipboard, ipcMain } from 'electron'
import { CHANNEL, type PtySize, type SessionId } from '@shared/ipc'
import type { PtySessions } from '../pty/sessions'

export function registerIpc(sessions: PtySessions, window: BrowserWindow): void {
  ipcMain.handle(CHANNEL.ptyCreate, (_event, size: PtySize) => sessions.create(size))
  ipcMain.on(CHANNEL.ptyWrite, (_event, id: SessionId, data: string) => sessions.write(id, data))
  ipcMain.on(CHANNEL.ptyResize, (_event, id: SessionId, size: PtySize) => sessions.resize(id, size))
  ipcMain.on(CHANNEL.ptyKill, (_event, id: SessionId) => sessions.kill(id))

  ipcMain.handle(CHANNEL.clipboardRead, () => clipboard.readText())
  ipcMain.on(CHANNEL.clipboardWrite, (_event, text: string) => clipboard.writeText(text))

  sessions.onData((event) => {
    if (window.isDestroyed()) {
      return
    }
    window.webContents.send(CHANNEL.ptyData, event)
  })

  sessions.onExit((event) => {
    if (window.isDestroyed()) {
      return
    }
    window.webContents.send(CHANNEL.ptyExit, event)
  })
}
