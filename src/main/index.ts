import { app } from 'electron'
import { paletteFor } from '@shared/theme'
import { registerAgents } from './agents'
import { applicationName } from './channel'
import { ConfigStore } from './config'
import { configDirectory } from './config/files'
import { registerFocus } from './focus'
import { registerIpc } from './ipc'
import { PtySessions } from './pty/sessions'
import type { Service } from './service'
import { SnapshotStore } from './snapshot/store'
import { registerUpdates } from './update'
import { ApplicationMenu } from './window/applicationMenu'
import { createMainWindow, isBackgroundWindow } from './window/window'

app.setName(applicationName)

const sessions = new PtySessions()
const services: Service[] = []
let snapshotStore: SnapshotStore | undefined
let flushed = false

app.whenReady().then(() => {
  // Launching a GUI app activates it and takes the user's focus, so a suite run
  // has to leave the dock alone.
  if (isBackgroundWindow()) {
    app.dock?.hide()
  }

  const applicationMenu = new ApplicationMenu()
  applicationMenu.show()

  const directory = configDirectory()
  const configStore = new ConfigStore(directory)
  const initialConfig = configStore.current()
  const palette = paletteFor(initialConfig.appearance)
  const window = createMainWindow(palette)
  const snapshots = new SnapshotStore(directory, sessions)
  snapshotStore = snapshots
  registerIpc(sessions, window, configStore, snapshots)
  const updates = registerUpdates(window, directory, applicationMenu)
  const agents = registerAgents(window, directory, configStore, applicationMenu, sessions)
  const focus = registerFocus(sessions, window, directory, snapshots, () => {
    updates.checkNow()
  })
  services.push(configStore, focus, updates, agents, snapshots)

  // Closing the window is the start of a quit, and the snapshot has to stop
  // changing from that moment, before the ptys go down with the process.
  window.on('close', () => {
    snapshots.freeze()
  })

  configStore.onChange((config) => {
    // The store keeps running after the window closes, until the snapshot's last write is done.
    if (window.isDestroyed()) {
      return
    }
    const nextPalette = paletteFor(config.appearance)
    window.setBackgroundColor(nextPalette.chrome.terminalBackground)
  })
})

// The shells need no teardown: each one exits when its pty master closes with
// the process.
function stopEverything(): void {
  for (const service of services) {
    service.stop()
  }
}

// The first before-quit is held back while the snapshot's final write runs; the
// app.quit() at the end of that write fires it a second time, which goes through.
app.on('before-quit', (event) => {
  if (flushed) {
    return
  }
  flushed = true

  if (snapshotStore === undefined) {
    stopEverything()
    return
  }

  event.preventDefault()
  snapshotStore.freeze()
  void snapshotStore.flushAtQuit().finally(() => {
    stopEverything()
    app.quit()
  })
})

// A terminal manager with no windows has nothing left to manage, so closing the
// window quits rather than leaving the app resident the way macOS usually does.
app.on('window-all-closed', () => {
  app.quit()
})
