import { app } from 'electron'
import { CHANNEL, type AgentSignalEvent, type SessionId } from '@protocol/ipc'
import type { AgentSignal } from '@protocol/rows'
import type { AgentSession } from '@workspace/snapshot/format'
import type { Workspace } from '@workspace/workspace'
import type { FocusEvents } from '../../focus'
import type { Service } from '../../service'
import type { WindowLink } from '../window/link'

export function bridgeFocus(
  link: WindowLink,
  workspace: Workspace,
  start: (events: FocusEvents) => Service
): Service {
  const events: FocusEvents = {
    onFocus: (id: SessionId) => {
      workspace.focusRequested(id)
    },
    onAttention: (id: SessionId) => {
      workspace.attentionRequested(id)
    },
    onAgentStart: (id: SessionId, session: AgentSession, hasConversation: boolean) => {
      workspace.agentStarted(id, session, hasConversation)
    },
    onAgentEnd: (id: SessionId, harness: string, sessionId: string) => {
      workspace.agentEnded(id, harness, sessionId)
    },
    onAgentSignal: (id: SessionId, harness: string, sessionId: string, signal: AgentSignal) => {
      const event: AgentSignalEvent = { id, harness, sessionId, signal }
      workspace.agentSignalled(event)
    },
    onUnavailable: () => {
      link.send(CHANNEL.focusUnavailable)
    }
  }
  const focus = start(events)

  const onActivate = (): void => {
    workspace.appActivated()
  }
  app.on('activate', onActivate)

  return {
    stop: () => {
      focus.stop()
      app.off('activate', onActivate)
    }
  }
}
