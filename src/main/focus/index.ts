import { app, type BrowserWindow } from 'electron'
import { CHANNEL, type ClaudeSignalEvent, type SessionId } from '@shared/ipc'
import type { ClaudeSignal } from '@shared/rowState'
import { focusPortPath } from '../config/files'
import type { ClaudeSession } from '@shared/snapshot'
import type { PtySessions } from '../pty/sessions'
import type { SnapshotStore } from '../snapshot/store'
import { startFocusServer } from './server'

function sendToWindow(window: BrowserWindow, channel: string, id: SessionId): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel, id)
}

function sendSignal(window: BrowserWindow, event: ClaudeSignalEvent): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(CHANNEL.claudeSignal, event)
}

function sendBare(window: BrowserWindow, channel: string): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel)
}

export function registerFocus(
  sessions: PtySessions,
  window: BrowserWindow,
  directory: string,
  snapshotStore: SnapshotStore
): () => void {
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
      onClaudeStart: (id: SessionId, session: ClaudeSession) => {
        snapshotStore.recordClaudeStart(id, session)
        sendSignal(window, { id, claudeSessionId: session.sessionId, signal: 'start' })
      },
      onClaudeEnd: (id: SessionId, sessionId: string) => {
        snapshotStore.recordClaudeEnd(id, sessionId)
        sendSignal(window, { id, claudeSessionId: sessionId, signal: 'end' })
      },
      onClaudeSignal: (id: SessionId, claudeSessionId: string, signal: ClaudeSignal) => {
        sendSignal(window, { id, claudeSessionId, signal })
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
