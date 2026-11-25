import { join } from 'node:path'
import { BrowserWindow } from 'electron'
import type { Palette } from '@protocol/theme'
import { TRAFFIC_LIGHT_POSITION } from '@settings/chrome'

export function isBackgroundWindow(): boolean {
  return process.env.TACO_SHELLS_BACKGROUND_WINDOW === '1'
}

export function createMainWindow(palette: Palette): BrowserWindow {
  const window = new BrowserWindow({
    width: 1100,
    height: 720,
    minWidth: 520,
    minHeight: 320,
    show: false,
    backgroundColor: palette.chrome.terminalBackground,
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: TRAFFIC_LIGHT_POSITION,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  })

  // The e2e suite runs a window per test and must never interrupt whoever is
  // typing, so its window is not shown at all.
  const background = isBackgroundWindow()

  window.once('ready-to-show', () => {
    if (background) {
      return
    }
    window.show()
  })

  // The window only ever shows the app: a dropped file or a followed link would replace it and strand every terminal.
  // Reloading the page it is already on stays allowed: the dev server does that after an edit.
  window.webContents.on('will-navigate', (event, url) => {
    const isReload = url === window.webContents.getURL()
    if (!isReload) {
      event.preventDefault()
    }
  })
  window.webContents.setWindowOpenHandler(() => {
    return { action: 'deny' }
  })

  const devServerUrl = process.env.ELECTRON_RENDERER_URL
  if (devServerUrl) {
    void window.loadURL(devServerUrl)
  } else {
    void window.loadFile(join(__dirname, '../renderer/index.html'))
  }

  return window
}
