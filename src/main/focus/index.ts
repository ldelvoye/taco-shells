import { app, type BrowserWindow } from 'electron'
import { CHANNEL, type AgentSignalEvent, type SessionId } from '@shared/ipc'
import type { AgentSignal } from '@shared/rowState'
import { focusPortPath } from '../config/files'
import type { AgentSession } from '@shared/snapshot'
import type { PtySessions } from '../pty/sessions'
import type { Service } from '../service'
import type { SnapshotStore } from '../snapshot/store'
import { sendToWindow } from '../window/send'
import { startFocusServer } from './server'

export function registerFocus(
  sessions: PtySessions,
  window: BrowserWindow,
  directory: string,
  snapshotStore: SnapshotStore,
  onUpdateCheck: () => void
): Service {
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
        const event: AgentSignalEvent = { id, harness: session.harness, sessionId: session.sessionId, signal }
        sendToWindow(window, CHANNEL.agentSignal, event)
      },
      onAgentEnd: (id: SessionId, harness: string, sessionId: string) => {
        snapshotStore.recordAgentEnd(id, harness, sessionId)
        const event: AgentSignalEvent = { id, harness, sessionId, signal: 'end' }
        sendToWindow(window, CHANNEL.agentSignal, event)
      },
      onAgentSignal: (id: SessionId, harness: string, sessionId: string, signal: AgentSignal) => {
        const event: AgentSignalEvent = { id, harness, sessionId, signal }
        sendToWindow(window, CHANNEL.agentSignal, event)
      },
      onUpdateCheck,
      onUnavailable: () => {
        sendToWindow(window, CHANNEL.focusUnavailable)
      }
    },
    portFilePath
  )

  const onActivate = (): void => {
    sendToWindow(window, CHANNEL.appActivated)
  }
  app.on('activate', onActivate)

  return {
    stop: () => {
      stopServer()
      app.off('activate', onActivate)
    }
  }
}
