import { Menu } from 'electron'
import { buildApplicationMenu, type AgentMenuEntry, type ReadyUpdate } from './menu'

/** The one owner of the application menu, rebuilt whenever an update or an agent's state changes. */
export class ApplicationMenu {
  private readyUpdate: ReadyUpdate | null = null
  private agents: AgentMenuEntry[] = []

  setReadyUpdate(readyUpdate: ReadyUpdate | null): void {
    this.readyUpdate = readyUpdate
    this.show()
  }

  setAgents(agents: AgentMenuEntry[]): void {
    this.agents = agents
    this.show()
  }

  show(): void {
    const menu = buildApplicationMenu({ readyUpdate: this.readyUpdate, agents: this.agents })
    Menu.setApplicationMenu(menu)
  }
}
