import { join } from 'node:path'
import { BrowserWindow } from 'electron'
import { TRAFFIC_LIGHT_POSITION } from '@shared/chrome'

const WINDOW_BACKGROUND = '#1e1e1e'

export function createMainWindow(): BrowserWindow {
  const window = new BrowserWindow({
    width: 1100,
    height: 720,
    minWidth: 520,
    minHeight: 320,
    show: false,
    backgroundColor: WINDOW_BACKGROUND,
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: TRAFFIC_LIGHT_POSITION,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  })

  window.once('ready-to-show', () => {
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
