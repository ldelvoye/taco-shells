import { clipboard, ipcMain, shell } from 'electron'
import { CHANNEL } from '@protocol/ipc'
import { isOpenableLink } from '../links'
import type { WindowLink } from '../window/link'

export function registerHostApi(link: WindowLink): void {
  ipcMain.handle(CHANNEL.clipboardRead, () => clipboard.readText())
  ipcMain.on(CHANNEL.clipboardWrite, (_event, text: string) => clipboard.writeText(text))

  ipcMain.on(CHANNEL.linkOpen, (_event, url: string) => {
    if (!isOpenableLink(url)) {
      return
    }
    void shell.openExternal(url)
  })

  // sendSync is answered by assigning returnValue, not by returning: a handler
  // that returns instead leaves the renderer waiting at startup.
  ipcMain.on(CHANNEL.fullScreenGet, (event) => {
    event.returnValue = link.isFullScreen()
  })

  link.onFullScreenChanged((fullScreen) => {
    link.send(CHANNEL.fullScreenChanged, fullScreen)
  })
}
