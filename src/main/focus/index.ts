import { app, type BrowserWindow } from 'electron'
import { CHANNEL, type SessionId } from '@shared/ipc'
import { focusPortPath } from '../config/files'
import type { PtySessions } from '../pty/sessions'
import { startFocusServer } from './server'

function sendToWindow(window: BrowserWindow, channel: string, id: SessionId): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel, id)
}

function sendBare(window: BrowserWindow, channel: string): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel)
}

export function registerFocus(sessions: PtySessions, window: BrowserWindow, directory: string): () => void {
  const portFilePath = focusPortPath(directory)

  const stopServer = startFocusServer(
    {
      sessionForPids: (pids: number[]) => sessions.sessionForPids(pids),
      onFocus: (id: SessionId) => {
        sendToWindow(window, CHANNEL.focusSession, id)
      },
      onAttention: (id: SessionId) => {
        sendToWindow(window, CHANNEL.attentionSession, id)
      },
      onUnavailable: () => {
        sendBare(window, CHANNEL.focusUnavailable)
      }
    },
    portFilePath
  )

  const onActivate = (): void => {
    sendBare(window, CHANNEL.appActivated)
  }
  app.on('activate', onActivate)

  return () => {
    stopServer()
    app.off('activate', onActivate)
  }
}
