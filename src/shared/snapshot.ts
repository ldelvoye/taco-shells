import type { SessionId } from './ipc'
import type { GroupId, Group, Pane, RowColor, TabGroup, TabGroupId, Workspace } from './model'
import { HARNESSES, harnessOf, LEGACY_HARNESS } from './harness/registry'
import type { Harness } from './harness/types'
import { emptyWorkspace, MIN_PANE_WIDTH, openedAt, ROW_COLORS } from './model'

// Tab groups were added without a new version: a file without them loads with none, and a build
// from before them ignores the fields.
export const SNAPSHOT_VERSION = 1

// The model's width floor caps a group at ten panes; a hand-edited file may not exceed it.
const MAX_PANES_PER_GROUP = Math.floor(1 / MIN_PANE_WIDTH)

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

type Loose = Record<string, unknown>

function isRecord(value: unknown): value is Loose {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function clampIndex(value: unknown, length: number): number {
  if (typeof value !== 'number' || !Number.isInteger(value)) {
    return 0
  }
  const lastIndex = Math.max(0, length - 1)
  const boundedBelow = Math.max(0, value)
  return Math.min(lastIndex, boundedBelow)
}

/** The session a value describes for one harness: an id that harness accepts and a non-empty directory, or null. */
export function agentSessionFor(harness: Harness, value: unknown): AgentSession | null {
  if (!isRecord(value)) {
    return null
  }
  const { sessionId, cwd } = value
  if (!harness.isSessionId(sessionId)) {
    return null
  }
  if (typeof cwd !== 'string' || cwd === '') {
    return null
  }
  return { harness: harness.id, sessionId, cwd }
}

/** The session a saved value describes, looked up by its harness id, or null. */
export function agentSessionFrom(
  value: unknown,
  harnesses: readonly Harness[] = HARNESSES
): AgentSession | null {
  if (!isRecord(value)) {
    return null
  }
  const harness = harnessOf(value.harness, harnesses)
  if (harness === null) {
    return null
  }
  return agentSessionFor(harness, value)
}

function rowColorFrom(value: unknown): RowColor | null {
  const knownColor = ROW_COLORS.find((color) => color === value)
  if (knownColor === undefined) {
    return null
  }
  return knownColor
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
    // A file saved before harnesses holds the session under "claude", with no harness id.
    let agent: AgentSession | null
    if (raw.agent !== undefined) {
      agent = agentSessionFrom(raw.agent)
    } else {
      agent = agentSessionFor(LEGACY_HARNESS, raw.claude)
    }
    const color = rowColorFrom(raw.color)
    return { width, cwd, agent, color }
  })

  const activePane = clampIndex(value.activePane, panes.length)
  const tabGroup = tabGroupIndexFrom(value.tabGroup)
  return { panes, activePane, tabGroup }
}

function tabGroupIndexFrom(value: unknown): number | null {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) {
    return null
  }
  return value
}

function parseTabGroup(value: unknown): SnapshotTabGroup {
  let loose: Loose = {}
  if (isRecord(value)) {
    loose = value
  }

  let name = ''
  if (typeof loose.name === 'string') {
    name = loose.name
  }

  let color: RowColor = ROW_COLORS[0]
  const knownColor = rowColorFrom(loose.color)
  if (knownColor !== null) {
    color = knownColor
  }

  let collapsed = false
  if (typeof loose.collapsed === 'boolean') {
    collapsed = loose.collapsed
  }

  return { name, color, collapsed }
}

function parseTabGroups(value: unknown): SnapshotTabGroup[] {
  if (!Array.isArray(value)) {
    return []
  }

  const candidates: unknown[] = value
  return candidates.map(parseTabGroup)
}

function withKnownTabGroups(groups: SnapshotGroup[], recordCount: number): SnapshotGroup[] {
  return groups.map((group) => {
    const index = group.tabGroup
    if (index !== null && index >= recordCount) {
      return { ...group, tabGroup: null }
    }
    return group
  })
}

// A hand-edited file can split a tab group across the list; its later pieces load in no group.
function withTabGroupsAdjacent(groups: SnapshotGroup[]): SnapshotGroup[] {
  const seen = new Set<number>()
  let previous: number | null = null

  return groups.map((group) => {
    let tabGroup = group.tabGroup
    if (tabGroup !== null) {
      const straggler = seen.has(tabGroup) && previous !== tabGroup
      if (straggler) {
        tabGroup = null
      } else {
        seen.add(tabGroup)
      }
    }

    previous = tabGroup
    if (tabGroup === group.tabGroup) {
      return group
    }
    return { ...group, tabGroup }
  })
}

function withoutUnusedTabGroups(
  groups: SnapshotGroup[],
  records: SnapshotTabGroup[]
): { groups: SnapshotGroup[]; tabGroups: SnapshotTabGroup[] } {
  const usedIndexes = new Set(groups.map((group) => group.tabGroup))
  const tabGroups = records.filter((_record, index) => usedIndexes.has(index))

  const renumbered = new Map<number, number>()
  records.forEach((_record, index) => {
    if (usedIndexes.has(index)) {
      renumbered.set(index, renumbered.size)
    }
  })

  const renumberedGroups = groups.map((group): SnapshotGroup => {
    if (group.tabGroup === null) {
      return group
    }

    const renumberedIndex = renumbered.get(group.tabGroup)
    if (renumberedIndex === undefined) {
      return { ...group, tabGroup: null }
    }
    return { ...group, tabGroup: renumberedIndex }
  })

  return { groups: renumberedGroups, tabGroups }
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
  const parsedKept = parsedGroups.filter((group): group is SnapshotGroup => group !== null)
  const activeGroup = clampIndex(parsed.activeGroup, parsedKept.length)

  const records = parseTabGroups(parsed.tabGroups)
  const known = withKnownTabGroups(parsedKept, records.length)
  const adjacent = withTabGroupsAdjacent(known)
  const { groups, tabGroups } = withoutUnusedTabGroups(adjacent, records)

  return { version: SNAPSHOT_VERSION, groups, activeGroup, tabGroups }
}

function tabGroupsInOrder(workspace: Workspace): TabGroup[] {
  const ordered: TabGroup[] = []
  for (const group of workspace.groups) {
    const id = group.tabGroup
    if (id !== null) {
      const alreadyListed = ordered.some((tabGroup) => tabGroup.id === id)
      const record = workspace.tabGroups.find((tabGroup) => tabGroup.id === id)
      if (!alreadyListed && record !== undefined) {
        ordered.push(record)
      }
    }
  }
  return ordered
}

function snapshotIndexOf(ordered: TabGroup[], id: TabGroupId | null): number | null {
  const index = ordered.findIndex((tabGroup) => tabGroup.id === id)
  if (id === null || index === -1) {
    return null
  }
  return index
}

/** The snapshot of a workspace, with each pane's directory and agent session looked up by id. */
export function snapshotFrom(
  workspace: Workspace,
  cwdOf: (id: SessionId) => string | null,
  agentOf: (id: SessionId) => AgentSession | null
): Snapshot {
  const ordered = tabGroupsInOrder(workspace)
  const groups = workspace.groups.map((group): SnapshotGroup => {
    const panes = group.panes.map((pane): SnapshotPane => {
      return {
        width: pane.width,
        cwd: cwdOf(pane.id),
        agent: agentOf(pane.id),
        color: pane.color
      }
    })
    const tabGroup = snapshotIndexOf(ordered, group.tabGroup)
    return { panes, activePane: group.activePane, tabGroup }
  })

  const tabGroups = ordered.map((record): SnapshotTabGroup => {
    return { name: record.name, color: record.color, collapsed: record.collapsed }
  })

  return { version: SNAPSHOT_VERSION, groups, activeGroup: workspace.activeGroup, tabGroups }
}

/** The total number of panes across all groups. */
export function paneCount(snapshot: Snapshot): number {
  return snapshot.groups.reduce((sum, group) => sum + group.panes.length, 0)
}

/** The workspace a normalised snapshot describes, or an empty one when the id counts do not fit. */
export function workspaceFromSnapshot(
  snapshot: Snapshot,
  sessionIds: SessionId[],
  groupIds: GroupId[],
  tabGroupIds: TabGroupId[]
): Workspace {
  if (sessionIds.length !== paneCount(snapshot)) {
    return emptyWorkspace()
  }
  if (groupIds.length !== snapshot.groups.length) {
    return emptyWorkspace()
  }
  if (tabGroupIds.length !== snapshot.tabGroups.length) {
    return emptyWorkspace()
  }

  let nextSession = 0
  const groups = snapshot.groups.map((snapshotGroup, groupIndex): Group => {
    const panes = snapshotGroup.panes.map((snapshotPane): Pane => {
      const id = sessionIds[nextSession]
      nextSession += 1
      return { id, title: '', width: snapshotPane.width, color: snapshotPane.color }
    })

    let tabGroup: TabGroupId | null = null
    if (snapshotGroup.tabGroup !== null) {
      tabGroup = tabGroupIds[snapshotGroup.tabGroup]
    }
    return { id: groupIds[groupIndex], panes, activePane: snapshotGroup.activePane, tabGroup }
  })

  const tabGroups = snapshot.tabGroups.map((record, index): TabGroup => {
    return { id: tabGroupIds[index], name: record.name, color: record.color, collapsed: record.collapsed }
  })

  const restored: Workspace = { groups, activeGroup: snapshot.activeGroup, tabGroups }
  return openedAt(restored, restored.activeGroup)
}
