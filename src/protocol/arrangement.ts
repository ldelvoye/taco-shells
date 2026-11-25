import type { SessionId } from './ipc'

export const ROW_COLORS = ['red', 'orange', 'yellow', 'green', 'blue', 'purple'] as const

export type GroupId = string

export type TabGroupId = string

export type RowColor = (typeof ROW_COLORS)[number]

export interface Pane {
  id: SessionId
  title: string
  /** Share of the group's width. A group's widths sum to 1. */
  width: number
  color: RowColor | null
}

export interface TabGroup {
  id: TabGroupId
  name: string
  color: RowColor
  collapsed: boolean
}

export interface Group {
  id: GroupId
  panes: Pane[]
  activePane: number
  tabGroup: TabGroupId | null
}

/**
 * The entries of one tab group sit next to each other in `groups`, every tab group has an entry,
 * and the active entry is not in a collapsed tab group unless every entry is.
 */
export interface Workspace {
  groups: Group[]
  activeGroup: number
  tabGroups: TabGroup[]
}

export type Dragged =
  | { kind: 'pane'; pane: SessionId }
  | { kind: 'group'; group: GroupId }
  | { kind: 'tabGroup'; tabGroup: TabGroupId }

/**
 * Where a drag lands. `index` and `slot` count gaps in the list as it is before the move, and
 * `tabGroup` is the tab group the dropped thing ends up in, or null for none.
 */
export type Drop =
  | { kind: 'between'; index: number; tabGroup: TabGroupId | null }
  | { kind: 'into'; group: GroupId; slot: number }
