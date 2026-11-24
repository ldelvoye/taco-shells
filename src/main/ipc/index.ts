import { type BrowserWindow, clipboard, ipcMain, shell } from 'electron'
import type { ConfigFile } from '@shared/config'
import { CHANNEL, type PtyCreateOptions, type PtySize, type SessionId } from '@shared/ipc'
import type { Workspace } from '@shared/model'
import type { ConfigStore } from '../config'
import { configFilePath } from '../config/files'
import { isOpenableLink } from '../links'
import type { PtySessions } from '../pty/sessions'
import { shellStartFor } from '../pty/start'
import type { SnapshotStore } from '../snapshot/store'
import { sendToWindow } from '../window/send'

export function registerIpc(
  sessions: PtySessions,
  window: BrowserWindow,
  store: ConfigStore,
  snapshotStore: SnapshotStore
): void {
  ipcMain.handle(
    CHANNEL.ptyCreate,
    async (_event, size: PtySize, options: PtyCreateOptions = {}) => {
      const agentSessionOf = (id: SessionId) => snapshotStore.agentSessionOf(id)
      const start = shellStartFor(options, agentSessionOf)
      const id = await sessions.create(size, start)
      const startingDirectory = sessions.startingDirectory(id)
      if (startingDirectory !== null) {
        snapshotStore.seedDirectory(id, startingDirectory)
      }
      // A fork is not seeded: its session id is unknown until its SessionStart hook reports.
      if (options.resume !== undefined) {
        snapshotStore.seedAgent(id, options.resume)
      }
      return id
    }
  )
  ipcMain.on(CHANNEL.ptyWrite, (_event, id: SessionId, data: string) => sessions.write(id, data))
  ipcMain.on(CHANNEL.ptyResize, (_event, id: SessionId, size: PtySize) => sessions.resize(id, size))
  ipcMain.on(CHANNEL.ptyKill, (_event, id: SessionId) => sessions.kill(id))

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
  ipcMain.on(CHANNEL.configGet, (event) => {
    event.returnValue = store.current()
  })
  ipcMain.on(CHANNEL.configPath, (event, which: ConfigFile) => {
    const directory = store.configDirectory()
    event.returnValue = configFilePath(directory, which)
  })
  ipcMain.on(CHANNEL.fullScreenGet, (event) => {
    event.returnValue = window.isFullScreen()
  })
  ipcMain.on(CHANNEL.snapshotGet, (event) => {
    if (store.current().settings.restoreOnLaunch) {
      event.returnValue = snapshotStore.initial()
    } else {
      event.returnValue = null
    }
  })
  ipcMain.on(CHANNEL.snapshotWorkspace, (_event, workspace: Workspace) => {
    snapshotStore.workspaceChanged(workspace)
  })

  window.on('enter-full-screen', () => {
    sendToWindow(window, CHANNEL.fullScreenChanged, true)
  })
  window.on('leave-full-screen', () => {
    sendToWindow(window, CHANNEL.fullScreenChanged, false)
  })

  sessions.onData((event) => {
    sendToWindow(window, CHANNEL.ptyData, event)
  })

  sessions.onExit((event) => {
    sendToWindow(window, CHANNEL.ptyExit, event)
  })

  store.onChange((config) => {
    sendToWindow(window, CHANNEL.configChanged, config)
  })
}
