import { app } from 'electron'
import { CHANNEL, type AgentSignalEvent, type SessionId } from '@protocol/ipc'
import type { FocusEvents } from '../../focus'
import type { Service } from '../../service'
import type { WindowLink } from '../window/link'

export function bridgeFocus(link: WindowLink, start: (events: FocusEvents) => Service): Service {
  const events: FocusEvents = {
    onFocus: (id: SessionId) => {
      link.send(CHANNEL.focusSession, id)
    },
    onAttention: (id: SessionId) => {
      link.send(CHANNEL.attentionSession, id)
    },
    onAgentSignal: (event: AgentSignalEvent) => {
      link.send(CHANNEL.agentSignal, event)
    },
    onUnavailable: () => {
      link.send(CHANNEL.focusUnavailable)
    }
  }
  const focus = start(events)

  const onActivate = (): void => {
    link.send(CHANNEL.appActivated)
  }
  app.on('activate', onActivate)

  return {
    stop: () => {
      focus.stop()
      app.off('activate', onActivate)
    }
  }
}
