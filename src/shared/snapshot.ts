import type { SessionId } from './ipc'
import type { GroupId, Group, Pane, Workspace } from './model'
import { emptyWorkspace, MIN_PANE_WIDTH } from './model'

export const SNAPSHOT_VERSION = 1

const SESSION_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
// The model's width floor caps a group at ten panes; a hand-edited file may not exceed it.
const MAX_PANES_PER_GROUP = Math.floor(1 / MIN_PANE_WIDTH)

export interface ClaudeSession {
  sessionId: string
  cwd: string
}

export interface SnapshotPane {
  width: number
  cwd: string | null
  claude: ClaudeSession | null
}

export interface SnapshotGroup {
  panes: SnapshotPane[]
  activePane: number
}

export interface Snapshot {
  version: typeof SNAPSHOT_VERSION
  groups: SnapshotGroup[]
  activeGroup: number
}

type Loose = Record<string, unknown>

function isRecord(value: unknown): value is Loose {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Whether a value is a Claude Code session id (a UUID string). */
export function isSessionUuid(value: unknown): value is string {
  if (typeof value !== 'string') {
    return false
  }
  return SESSION_UUID.test(value)
}

function clampIndex(value: unknown, length: number): number {
  if (typeof value !== 'number' || !Number.isInteger(value)) {
    return 0
  }
  const lastIndex = Math.max(0, length - 1)
  const boundedBelow = Math.max(0, value)
  return Math.min(lastIndex, boundedBelow)
}

/** The Claude session a value describes: a UUID id and a non-empty directory, or null. */
export function claudeSessionFrom(value: unknown): ClaudeSession | null {
  if (!isRecord(value)) {
    return null
  }
  const { sessionId, cwd } = value
  if (!isSessionUuid(sessionId)) {
    return null
  }
  if (typeof cwd !== 'string' || cwd === '') {
    return null
  }
  return { sessionId, cwd }
}

function parseGroup(value: unknown): SnapshotGroup | null {
  if (!isRecord(value) || !Array.isArray(value.panes)) {
    return null
  }

  const recordPanes = value.panes.filter(isRecord)
  const rawPanes = recordPanes.slice(0, MAX_PANES_PER_GROUP)
  if (rawPanes.length === 0) {
    return null
  }

  const equalShare = 1 / rawPanes.length
  const widths = rawPanes.map((raw) => {
    const width = raw.width
    if (typeof width === 'number' && Number.isFinite(width) && width > 0) {
      return width
    }
    return equalShare
  })
  const widthSum = widths.reduce((sum, width) => sum + width, 0)

  const panes = rawPanes.map((raw, index): SnapshotPane => {
    let cwd: string | null = null
    if (typeof raw.cwd === 'string') {
      cwd = raw.cwd
    }
    const width = widths[index] / widthSum
    const claude = claudeSessionFrom(raw.claude)
    return { width, cwd, claude }
  })

  const activePane = clampIndex(value.activePane, panes.length)
  return { panes, activePane }
}

/** The snapshot a file's text describes, or null when it cannot be one. */
export function parseSnapshot(text: string): Snapshot | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch {
    return null
  }

  if (!isRecord(parsed) || parsed.version !== SNAPSHOT_VERSION) {
    return null
  }
  if (!Array.isArray(parsed.groups)) {
    return null
  }

  const candidates: unknown[] = parsed.groups
  const parsedGroups = candidates.map(parseGroup)
  const groups = parsedGroups.filter((group): group is SnapshotGroup => group !== null)
  const activeGroup = clampIndex(parsed.activeGroup, groups.length)

  return { version: SNAPSHOT_VERSION, groups, activeGroup }
}

/** The snapshot of a workspace, with each pane's directory and Claude session looked up by id. */
export function snapshotFrom(
  workspace: Workspace,
  cwdOf: (id: SessionId) => string | null,
  claudeOf: (id: SessionId) => ClaudeSession | null
): Snapshot {
  const groups = workspace.groups.map((group): SnapshotGroup => {
    const panes = group.panes.map((pane): SnapshotPane => {
      return { width: pane.width, cwd: cwdOf(pane.id), claude: claudeOf(pane.id) }
    })
    return { panes, activePane: group.activePane }
  })

  return { version: SNAPSHOT_VERSION, groups, activeGroup: workspace.activeGroup }
}

/** The total number of panes across all groups. */
export function paneCount(snapshot: Snapshot): number {
  return snapshot.groups.reduce((sum, group) => sum + group.panes.length, 0)
}

/** The workspace a normalised snapshot describes, or an empty one when the id counts do not fit. */
export function workspaceFromSnapshot(
  snapshot: Snapshot,
  sessionIds: SessionId[],
  groupIds: GroupId[]
): Workspace {
  if (sessionIds.length !== paneCount(snapshot)) {
    return emptyWorkspace()
  }
  if (groupIds.length !== snapshot.groups.length) {
    return emptyWorkspace()
  }

  let nextSession = 0
  const groups = snapshot.groups.map((snapshotGroup, groupIndex): Group => {
    const panes = snapshotGroup.panes.map((snapshotPane): Pane => {
      const id = sessionIds[nextSession]
      nextSession += 1
      return { id, title: '', width: snapshotPane.width }
    })
    return { id: groupIds[groupIndex], panes, activePane: snapshotGroup.activePane }
  })

  return { groups, activeGroup: snapshot.activeGroup }
}
