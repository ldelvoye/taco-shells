import { ipcMain } from 'electron'
import type { ConfigFile } from '@protocol/config'
import { CHANNEL } from '@protocol/ipc'
import type { ConfigStore } from '../../config'
import { configFilePath } from '../../config/files'
import type { RuntimeProcess } from '../runtime/process'
import type { WindowLink } from '../window/link'

export function registerIpc(runtimeProcess: RuntimeProcess, link: WindowLink, store: ConfigStore): void {
  ipcMain.on(CHANNEL.runtimeConnect, (event) => {
    runtimeProcess.connectWindow(event.sender)
  })

  // sendSync is answered by assigning returnValue, not by returning: a handler
  // that returns instead leaves the renderer waiting at startup.
  ipcMain.on(CHANNEL.configGet, (event) => {
    event.returnValue = store.current()
  })
  ipcMain.on(CHANNEL.configPath, (event, which: ConfigFile) => {
    const directory = store.configDirectory()
    event.returnValue = configFilePath(directory, which)
  })

  store.onChange((config) => {
    link.send(CHANNEL.configChanged, config)
  })
}
