import type { SessionId } from '@protocol/ipc'
import type { Runtime } from '@protocol/runtime'
import { focusPortPath } from '../config/files'
import type { Service } from '../service'
import { startFocusServer, type FocusServerHandlers } from './server'

export type FocusEvents = Omit<FocusServerHandlers, 'sessionForPids' | 'onUpdateCheck'>

export function startFocus(
  runtime: Runtime,
  directory: string,
  events: FocusEvents,
  onUpdateCheck: () => void
): Service {
  const portFilePath = focusPortPath(directory)

  const stopServer = startFocusServer(
    {
      sessionForPids: (pids: number[]) => runtime.terminalForPids(pids),
      onFocus: (id: SessionId) => {
        events.onFocus(id)
      },
      onAttention: (id: SessionId) => {
        events.onAttention(id)
      },
      onAgentStart: (id, session, hasConversation) => {
        events.onAgentStart(id, session, hasConversation)
      },
      onAgentEnd: (id, harness, sessionId) => {
        events.onAgentEnd(id, harness, sessionId)
      },
      onAgentSignal: (id, harness, sessionId, signal) => {
        events.onAgentSignal(id, harness, sessionId, signal)
      },
      onUpdateCheck,
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
