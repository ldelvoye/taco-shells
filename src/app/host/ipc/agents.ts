import { app, ipcMain } from 'electron'
import { CHANNEL } from '@protocol/ipc'
import type { AgentsService } from '../../agents'
import type { AgentSetupsEvents } from '../../agents/setups'
import type { Service } from '../../service'
import type { ApplicationMenu } from '../window/applicationMenu'
import type { WindowLink } from '../window/link'

function onAgent(channel: string, handle: (agentId: string) => void): void {
  ipcMain.on(channel, (_event, id: unknown) => {
    if (typeof id === 'string') {
      handle(id)
    }
  })
}

export function bridgeAgents(
  link: WindowLink,
  menu: ApplicationMenu,
  start: (events: AgentSetupsEvents) => AgentsService
): Service {
  const events: AgentSetupsEvents = {
    onMenuChanged: (entries) => {
      menu.setAgents(entries)
    },
    onBannersChanged: (banners) => {
      link.send(CHANNEL.agentsChanged, banners)
    }
  }
  const agents = start(events)

  // A sendSync left unanswered hangs the renderer at startup, so this answers
  // even when no harness declares a setup.
  ipcMain.on(CHANNEL.agentsGet, (event) => {
    event.returnValue = agents.banners()
  })

  onAgent(CHANNEL.agentsSetUp, (agentId) => {
    agents.setUp(agentId)
  })

  onAgent(CHANNEL.agentsDecline, (agentId) => {
    agents.decline(agentId)
  })

  onAgent(CHANNEL.agentsDismiss, (agentId) => {
    agents.dismiss(agentId)
  })

  const onActive = (): void => {
    agents.refresh()
  }
  app.on('did-become-active', onActive)

  return {
    stop: () => {
      agents.stop()
      app.off('did-become-active', onActive)
      ipcMain.removeAllListeners(CHANNEL.agentsGet)
      ipcMain.removeAllListeners(CHANNEL.agentsSetUp)
      ipcMain.removeAllListeners(CHANNEL.agentsDecline)
      ipcMain.removeAllListeners(CHANNEL.agentsDismiss)
    }
  }
}
