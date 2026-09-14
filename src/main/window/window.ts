import { join } from 'node:path'
import { BrowserWindow } from 'electron'
import { TRAFFIC_LIGHT_POSITION } from '@shared/chrome'
import type { Palette } from '@shared/theme'

export function isBackgroundWindow(): boolean {
  return process.env.TAQUERIA_BACKGROUND_WINDOW === '1'
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

  const devServerUrl = process.env.ELECTRON_RENDERER_URL
  if (devServerUrl) {
    void window.loadURL(devServerUrl)
  } else {
    void window.loadFile(join(__dirname, '../renderer/index.html'))
  }

  return window
}
