import type { CommandRegistry } from './registry'

export interface WorkspaceActions {
  openTerminal: () => void
  closeActiveTerminal: () => void
  toggleSidebar: () => void
}

export function registerWorkspaceCommands(
  registry: CommandRegistry,
  actions: WorkspaceActions
): void {
  registry.register('terminal.new', actions.openTerminal)
  registry.register('terminal.close', actions.closeActiveTerminal)
  registry.register('sidebar.toggle', actions.toggleSidebar)
}
