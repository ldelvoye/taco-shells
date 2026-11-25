import type { BrowserWindow } from 'electron'

/** A message to the renderer, dropped once the window is gone. */
export function sendToWindow(window: BrowserWindow, channel: string, ...payload: unknown[]): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel, ...payload)
}
