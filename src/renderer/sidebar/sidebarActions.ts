import type { SessionId } from '@shared/ipc'
import type { Dragged, Drop, RowColor, TabGroupId } from '@shared/model'

export interface SidebarActions {
  select(pane: SessionId): void
  close(pane: SessionId): void
  openTerminalIn(tabGroup: TabGroupId | null): void
  drop(dragged: Dragged, drop: Drop): void
  colorPane(pane: SessionId, color: RowColor | null): void
  fork(source: SessionId): void
  newTabGroup(pane: SessionId): TabGroupId | null
  renameTabGroup(id: TabGroupId, name: string): void
  recolorTabGroup(id: TabGroupId, color: RowColor): void
  ungroupTabGroup(id: TabGroupId): void
  toggleTabGroup(id: TabGroupId): void
  refocusActive(): void
}
