import { app } from 'electron'
import { CHANNEL, type AgentSignalEvent, type SessionId } from '@protocol/ipc'
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
    onAgentSignal: (event: AgentSignalEvent) => {
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
