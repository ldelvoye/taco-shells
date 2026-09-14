import { app, Menu } from 'electron'
import { paletteFor } from '@shared/theme'
import { ConfigStore } from './config'
import { configDirectory } from './config/files'
import { registerIpc } from './ipc'
import { PtySessions } from './pty/sessions'
import { buildApplicationMenu } from './window/menu'
import { createMainWindow, isBackgroundWindow } from './window/window'

app.setName('Taco Shells')

const sessions = new PtySessions()
let store: ConfigStore | undefined

app.whenReady().then(() => {
  // Launching a GUI app activates it and takes the user's focus, so a suite run
  // has to leave the dock alone.
  if (isBackgroundWindow()) {
    app.dock?.hide()
  }

  Menu.setApplicationMenu(buildApplicationMenu())

  const directory = configDirectory()
  store = new ConfigStore(directory)
  const initialConfig = store.current()
  const palette = paletteFor(initialConfig.appearance)
  const window = createMainWindow(palette)
  registerIpc(sessions, window, store)

  store.onChange((config) => {
    const nextPalette = paletteFor(config.appearance)
    window.setBackgroundColor(nextPalette.chrome.terminalBackground)
  })
})

// The shells need no teardown: each one exits when its pty master closes with
// the process.
app.on('before-quit', () => {
  if (store) {
    store.stop()
  }
})

// A terminal manager with no windows has nothing left to manage, so closing the
// window quits rather than leaving the app resident the way macOS usually does.
app.on('window-all-closed', () => {
  app.quit()
})
