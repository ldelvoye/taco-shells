import { app, type BrowserWindow } from 'electron'
import { CHANNEL, type AgentSignalEvent, type SessionId } from '@shared/ipc'
import type { AgentSignal } from '@shared/rowState'
import { focusPortPath } from '../config/files'
import type { AgentSession } from '@shared/snapshot'
import type { PtySessions } from '../pty/sessions'
import type { SnapshotStore } from '../snapshot/store'
import { startFocusServer } from './server'

function sendToWindow(window: BrowserWindow, channel: string, id: SessionId): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel, id)
}

function sendSignal(window: BrowserWindow, event: AgentSignalEvent): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(CHANNEL.agentSignal, event)
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
  snapshotStore: SnapshotStore,
  onUpdateCheck: () => void
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
      onAgentStart: (id: SessionId, session: AgentSession, hasConversation: boolean) => {
        snapshotStore.recordAgentStart(id, session)
        let signal: AgentSignal = 'start'
        if (hasConversation) {
          signal = 'resume'
        }
        sendSignal(window, { id, harness: session.harness, sessionId: session.sessionId, signal })
      },
      onAgentEnd: (id: SessionId, harness: string, sessionId: string) => {
        snapshotStore.recordAgentEnd(id, sessionId)
        sendSignal(window, { id, harness, sessionId, signal: 'end' })
      },
      onAgentSignal: (id: SessionId, harness: string, sessionId: string, signal: AgentSignal) => {
        sendSignal(window, { id, harness, sessionId, signal })
      },
      onUpdateCheck,
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
