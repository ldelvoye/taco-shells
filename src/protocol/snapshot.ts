import type { RowColor } from './arrangement'

// Tab groups were added without a new version: a file without them loads with none, and a build
// from before them ignores the fields.
export const SNAPSHOT_VERSION = 1

export interface AgentSession {
  harness: string
  sessionId: string
  cwd: string
}

export interface SnapshotPane {
  width: number
  cwd: string | null
  agent: AgentSession | null
  color: RowColor | null
}

export interface SnapshotTabGroup {
  name: string
  color: RowColor
  collapsed: boolean
}

export interface SnapshotGroup {
  panes: SnapshotPane[]
  activePane: number
  tabGroup: number | null
}

export interface Snapshot {
  version: typeof SNAPSHOT_VERSION
  groups: SnapshotGroup[]
  activeGroup: number
  tabGroups: SnapshotTabGroup[]
}
