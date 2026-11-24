import { app, Menu, type MenuItemConstructorOptions } from 'electron'
import type { AgentMenu } from '@shared/agentSetup'

const AGENTS_LABEL = 'Agents'
const SHOW_SCRIPTS_LABEL = 'Show Hook Scripts in Finder'

export interface ReadyUpdate {
  version: string
  onApply: () => void
}

export interface AgentMenuEntry {
  name: string
  menu: AgentMenu
  onAction: () => void
  onShowScripts: () => void
}

export interface ApplicationMenuState {
  readyUpdate: ReadyUpdate | null
  agents: AgentMenuEntry[]
}

function agentSubmenu(entry: AgentMenuEntry): MenuItemConstructorOptions {
  // A header reads as a caption, where a disabled item reads as an action that is unavailable.
  const status: MenuItemConstructorOptions = { type: 'header', label: entry.menu.status }
  const action: MenuItemConstructorOptions = {
    label: entry.menu.actionLabel,
    enabled: entry.menu.enabled,
    click: entry.onAction
  }
  if (entry.menu.note !== null) {
    action.sublabel = entry.menu.note
  }
  const showScripts: MenuItemConstructorOptions = { label: SHOW_SCRIPTS_LABEL, click: entry.onShowScripts }
  const submenu: MenuItemConstructorOptions[] = [status, action, { type: 'separator' }, showScripts]
  return { label: entry.name, submenu }
}

// No Edit menu: its copy and paste roles would claim cmd+c and cmd+v, and a menu
// accelerator fires before the renderer sees the keydown.
// No Close item: its cmd+w has to stay free for closing a terminal.
export function buildApplicationMenu(state: ApplicationMenuState): Menu {
  const template: MenuItemConstructorOptions[] = [
    {
      label: app.name,
      submenu: [
        { role: 'about' },
        { type: 'separator' },
        { role: 'hide' },
        { role: 'hideOthers' },
        { role: 'unhide' },
        { type: 'separator' },
        { role: 'quit' }
      ]
    },
    {
      label: 'View',
      submenu: [{ role: 'toggleDevTools' }]
    }
  ]

  if (state.agents.length > 0) {
    const agentSubmenus = state.agents.map(agentSubmenu)
    template.push({ label: AGENTS_LABEL, submenu: agentSubmenus })
  }

  template.push({
    label: 'Window',
    submenu: [{ role: 'minimize' }, { role: 'zoom' }]
  })

  const readyUpdate = state.readyUpdate
  if (readyUpdate !== null) {
    template.push({
      label: 'Update',
      submenu: [
        {
          label: `Restart to Update (${readyUpdate.version})`,
          click: readyUpdate.onApply
        }
      ]
    })
  }

  return Menu.buildFromTemplate(template)
}
