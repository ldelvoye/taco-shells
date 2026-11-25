import { ipcMain } from 'electron'
import type { ConfigFile } from '@protocol/config'
import { CHANNEL, type PtyDataEvent, type PtySize, type SessionId } from '@protocol/ipc'
import type { PtySessions } from '@runtime/sessions'
import type { ConfigStore } from '../../config'
import { configFilePath } from '../../config/files'
import type { WindowLink } from '../window/link'

export function registerIpc(sessions: PtySessions, link: WindowLink, store: ConfigStore): void {
  ipcMain.on(CHANNEL.ptyWrite, (_event, id: SessionId, data: string) => sessions.write(id, data))
  ipcMain.on(CHANNEL.ptyResize, (_event, id: SessionId, size: PtySize) => sessions.resize(id, size))

  // sendSync is answered by assigning returnValue, not by returning: a handler
  // that returns instead leaves the renderer waiting at startup.
  ipcMain.on(CHANNEL.configGet, (event) => {
    event.returnValue = store.current()
  })
  ipcMain.on(CHANNEL.configPath, (event, which: ConfigFile) => {
    const directory = store.configDirectory()
    event.returnValue = configFilePath(directory, which)
  })

  // The shells start before the page has loaded, and a message to a page that is
  // not listening yet is dropped: the first prompt would be lost.
  let held: PtyDataEvent[] | null = []

  ipcMain.on(CHANNEL.ptyListening, () => {
    const backlog = held
    held = null
    if (backlog === null) {
      return
    }
    for (const event of backlog) {
      link.send(CHANNEL.ptyData, event)
    }
  })

  sessions.onData((event) => {
    if (held !== null) {
      held.push(event)
      return
    }
    link.send(CHANNEL.ptyData, event)
  })

  sessions.onExit((event) => {
    link.send(CHANNEL.ptyExit, event)
  })

  store.onChange((config) => {
    link.send(CHANNEL.configChanged, config)
  })
}
