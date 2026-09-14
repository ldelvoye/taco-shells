import { type BrowserWindow, clipboard, ipcMain } from 'electron'
import type { ConfigFile } from '@shared/config'
import { CHANNEL, type PtySize, type SessionId } from '@shared/ipc'
import type { ConfigStore } from '../config'
import { configFilePath } from '../config/files'
import type { PtySessions } from '../pty/sessions'

export function registerIpc(sessions: PtySessions, window: BrowserWindow, store: ConfigStore): void {
  ipcMain.handle(CHANNEL.ptyCreate, (_event, size: PtySize, cwdFrom?: SessionId) =>
    sessions.create(size, cwdFrom)
  )
  ipcMain.on(CHANNEL.ptyWrite, (_event, id: SessionId, data: string) => sessions.write(id, data))
  ipcMain.on(CHANNEL.ptyResize, (_event, id: SessionId, size: PtySize) => sessions.resize(id, size))
  ipcMain.on(CHANNEL.ptyKill, (_event, id: SessionId) => sessions.kill(id))

  ipcMain.handle(CHANNEL.clipboardRead, () => clipboard.readText())
  ipcMain.on(CHANNEL.clipboardWrite, (_event, text: string) => clipboard.writeText(text))

  // sendSync is answered by assigning returnValue, not by returning: a handler
  // that returns instead leaves the renderer waiting at startup.
  ipcMain.on(CHANNEL.configGet, (event) => {
    event.returnValue = store.current()
  })
  ipcMain.on(CHANNEL.configPath, (event, which: ConfigFile) => {
    const directory = store.configDirectory()
    event.returnValue = configFilePath(directory, which)
  })

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

  store.onChange((config) => {
    if (window.isDestroyed()) {
      return
    }
    window.webContents.send(CHANNEL.configChanged, config)
  })
}
