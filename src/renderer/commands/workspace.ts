import { ROW_COLOR_COMMANDS } from '@shared/commands'
import { ROW_COLORS, type RowColor } from '@shared/model'
import type { CommandRegistry } from './registry'

export interface WorkspaceActions {
  openTerminal: () => void
  splitTerminal: () => void
  closeActiveTerminal: () => void
  focusNextPane: () => void
  focusPreviousPane: () => void
  focusNextGroup: () => void
  focusPreviousGroup: () => void
  toggleActiveColor: (color: RowColor) => void
  toggleSidebar: () => void
  openSettings: () => void
  openKeybindings: () => void
}

export function registerWorkspaceCommands(
  registry: CommandRegistry,
  actions: WorkspaceActions
): void {
  registry.register('terminal.new', actions.openTerminal)
  registry.register('terminal.split', actions.splitTerminal)
  registry.register('terminal.close', actions.closeActiveTerminal)
  registry.register('terminal.focusNextPane', actions.focusNextPane)
  registry.register('terminal.focusPreviousPane', actions.focusPreviousPane)
  registry.register('terminal.focusNextGroup', actions.focusNextGroup)
  registry.register('terminal.focusPreviousGroup', actions.focusPreviousGroup)
  registry.register('sidebar.toggle', actions.toggleSidebar)
  registry.register('config.openSettings', actions.openSettings)
  registry.register('config.openKeybindings', actions.openKeybindings)

  for (const color of ROW_COLORS) {
    registry.register(ROW_COLOR_COMMANDS[color], () => {
      actions.toggleActiveColor(color)
    })
  }
}
