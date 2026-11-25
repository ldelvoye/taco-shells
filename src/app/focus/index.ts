import type { AgentSignalEvent, SessionId } from '@protocol/ipc'
import type { AgentSignal } from '@protocol/rows'
import type { AgentSession } from '@protocol/snapshot'
import type { PtySessions } from '@runtime/sessions'
import type { SnapshotStore } from '@workspace/snapshot/store'
import { focusPortPath } from '../config/files'
import type { Service } from '../service'
import { startFocusServer } from './server'

export interface FocusEvents {
  onFocus(id: SessionId): void
  onAttention(id: SessionId): void
  onAgentSignal(event: AgentSignalEvent): void
  onUnavailable(): void
  onUpdateCheck(): void
}

export function startFocus(
  sessions: PtySessions,
  directory: string,
  snapshotStore: SnapshotStore,
  events: FocusEvents
): Service {
  const portFilePath = focusPortPath(directory)

  const stopServer = startFocusServer(
    {
      sessionForPids: (pids: number[]) => sessions.sessionForPids(pids),
      onFocus: (id: SessionId) => {
        events.onFocus(id)
      },
      onAttention: (id: SessionId) => {
        events.onAttention(id)
      },
      onAgentStart: (id: SessionId, session: AgentSession, hasConversation: boolean) => {
        snapshotStore.recordAgentStart(id, session)
        let signal: AgentSignal = 'start'
        if (hasConversation) {
          signal = 'resume'
        }
        const event: AgentSignalEvent = { id, harness: session.harness, sessionId: session.sessionId, signal }
        events.onAgentSignal(event)
      },
      onAgentEnd: (id: SessionId, harness: string, sessionId: string) => {
        snapshotStore.recordAgentEnd(id, harness, sessionId)
        const event: AgentSignalEvent = { id, harness, sessionId, signal: 'end' }
        events.onAgentSignal(event)
      },
      onAgentSignal: (id: SessionId, harness: string, sessionId: string, signal: AgentSignal) => {
        const event: AgentSignalEvent = { id, harness, sessionId, signal }
        events.onAgentSignal(event)
      },
      onUpdateCheck: () => {
        events.onUpdateCheck()
      },
      onUnavailable: () => {
        events.onUnavailable()
      }
    },
    portFilePath
  )

  return {
    stop: () => {
      stopServer()
    }
  }
}
