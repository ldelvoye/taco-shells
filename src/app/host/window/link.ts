import type { BrowserWindow } from 'electron'

/** The only handle the rest of the app has on the window. */
export interface WindowLink {
  /** A message to the renderer, dropped once the window is gone. */
  send(channel: string, ...payload: unknown[]): void
  isFullScreen(): boolean
  onFullScreenChanged(listener: (fullScreen: boolean) => void): void
  onClosing(listener: () => void): void
  setBackgroundColor(color: string): void
  close(): void
}

export function linkTo(window: BrowserWindow): WindowLink {
  return {
    send: (channel: string, ...payload: unknown[]) => {
      if (window.isDestroyed()) {
        return
      }
      window.webContents.send(channel, ...payload)
    },
    isFullScreen: () => window.isFullScreen(),
    onFullScreenChanged: (listener: (fullScreen: boolean) => void) => {
      window.on('enter-full-screen', () => {
        listener(true)
      })
      window.on('leave-full-screen', () => {
        listener(false)
      })
    },
    onClosing: (listener: () => void) => {
      window.on('close', () => {
        listener()
      })
    },
    setBackgroundColor: (color: string) => {
      // A config change can still arrive after the window closes, while the snapshot's last write runs.
      if (window.isDestroyed()) {
        return
      }
      window.setBackgroundColor(color)
    },
    close: () => {
      if (window.isDestroyed()) {
        return
      }
      window.close()
    }
  }
}
