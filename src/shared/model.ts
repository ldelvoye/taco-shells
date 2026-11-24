import type { SessionId } from './ipc'

export const MIN_PANE_WIDTH = 0.1

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

interface PaneLocation {
  groupIndex: number
  paneIndex: number
}

export function emptyWorkspace(): Workspace {
  return { groups: [], activeGroup: 0, tabGroups: [] }
}

/** The group and pane index of the pane with this id, or null when it is not open. */
export function locatePane(workspace: Workspace, id: SessionId): PaneLocation | null {
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === id)
  )
  if (groupIndex === -1) {
    return null
  }

  const group = workspace.groups[groupIndex]
  const paneIndex = group.panes.findIndex((pane) => pane.id === id)
  return { groupIndex, paneIndex }
}

/** The workspace with the group at this index replaced. */
export function withGroupAt(workspace: Workspace, index: number, group: Group): Workspace {
  const groups = spliced(workspace.groups, index, 1, [group])
  return { ...workspace, groups }
}

/** A copy of the list with removeCount items at index replaced by inserted. */
export function spliced<T>(
  items: readonly T[],
  index: number,
  removeCount: number,
  inserted: readonly T[]
): T[] {
  const before = items.slice(0, index)
  const after = items.slice(index + removeCount)
  return [...before, ...inserted, ...after]
}

/** The first and last index of a tab group's entries, or null when it has none. */
export function blockOf(
  workspace: Workspace,
  id: TabGroupId
): { first: number; last: number } | null {
  let first = -1
  let last = -1
  workspace.groups.forEach((group, index) => {
    if (group.tabGroup === id) {
      if (first === -1) {
        first = index
      }
      last = index
    }
  })

  if (first === -1) {
    return null
  }
  return { first, last }
}

/** Whether the entry at this index sits in a collapsed tab group. */
export function isHidden(workspace: Workspace, groupIndex: number): boolean {
  const group = workspace.groups[groupIndex]
  if (!group || group.tabGroup === null) {
    return false
  }

  const tabGroupId = group.tabGroup
  const record = workspace.tabGroups.find((tabGroup) => tabGroup.id === tabGroupId)
  if (record === undefined) {
    return false
  }
  return record.collapsed
}

function withoutEmptyTabGroups(workspace: Workspace): Workspace {
  const usedIds = new Set(workspace.groups.map((group) => group.tabGroup))
  const tabGroups = workspace.tabGroups.filter((tabGroup) => usedIds.has(tabGroup.id))
  if (tabGroups.length === workspace.tabGroups.length) {
    return workspace
  }

  return { ...workspace, tabGroups }
}

function nearestShowingEntry(workspace: Workspace): number | null {
  const lastIndex = workspace.groups.length - 1
  for (let index = workspace.activeGroup + 1; index <= lastIndex; index += 1) {
    if (!isHidden(workspace, index)) {
      return index
    }
  }
  for (let index = workspace.activeGroup - 1; index >= 0; index -= 1) {
    if (!isHidden(workspace, index)) {
      return index
    }
  }
  return null
}

function withFocusShowing(workspace: Workspace): Workspace {
  if (!isHidden(workspace, workspace.activeGroup)) {
    return workspace
  }

  const showing = nearestShowingEntry(workspace)
  if (showing === null) {
    return workspace
  }
  return { ...workspace, activeGroup: showing }
}

/** The workspace with empty tab groups dropped and focus moved off a hidden entry. */
export function settled(workspace: Workspace): Workspace {
  const pruned = withoutEmptyTabGroups(workspace)
  return withFocusShowing(pruned)
}

/** The workspace with the tab group of the entry at this index opened, if it was collapsed. */
export function openedAt(workspace: Workspace, groupIndex: number): Workspace {
  if (!isHidden(workspace, groupIndex)) {
    return workspace
  }

  const tabGroupId = workspace.groups[groupIndex].tabGroup
  const tabGroups = workspace.tabGroups.map((tabGroup) => {
    if (tabGroup.id === tabGroupId) {
      return { ...tabGroup, collapsed: false }
    }
    return tabGroup
  })

  return { ...workspace, tabGroups }
}

/** The workspace with a new active entry: last in the active entry's tab group, or last in the list when it has none. */
export function addGroup(workspace: Workspace, group: Group): Workspace {
  const anchor = workspace.groups[workspace.activeGroup]
  let tabGroup: TabGroupId | null = null
  if (anchor !== undefined) {
    tabGroup = anchor.tabGroup
  }
  return addGroupTo(workspace, group, tabGroup)
}

/** The workspace with a new active entry last in this tab group, or last and ungrouped when the group is null or gone. */
export function addGroupTo(
  workspace: Workspace,
  group: Group,
  tabGroup: TabGroupId | null
): Workspace {
  let placedIn: TabGroupId | null = null
  let index = workspace.groups.length
  if (tabGroup !== null) {
    const block = blockOf(workspace, tabGroup)
    if (block !== null) {
      placedIn = tabGroup
      index = block.last + 1
    }
  }

  const entry: Group = { ...group, tabGroup: placedIn }
  const groups = spliced(workspace.groups, index, 0, [entry])

  const added: Workspace = { ...workspace, groups, activeGroup: index }
  return openedAt(added, index)
}

// Where an active index lands once the entry at `removed` is taken out.
function indexAfterRemoval(active: number, removed: number, remaining: number): number {
  if (removed < active) {
    return active - 1
  }
  if (removed > active) {
    return active
  }

  const lastIndex = remaining - 1
  const withinList = Math.min(active, lastIndex)
  return Math.max(0, withinList)
}

export function closePane(workspace: Workspace, id: SessionId): Workspace {
  const location = locatePane(workspace, id)
  if (location === null) {
    return workspace
  }

  const { groupIndex, paneIndex } = location
  const group = workspace.groups[groupIndex]
  const panes = spliced(group.panes, paneIndex, 1, [])

  if (panes.length === 0) {
    const groups = spliced(workspace.groups, groupIndex, 1, [])
    const activeGroup = indexAfterRemoval(workspace.activeGroup, groupIndex, groups.length)

    return settled({ ...workspace, groups, activeGroup })
  }

  const survivorWidths = panes.map((pane) => pane.width)
  const survivorWidthSum = survivorWidths.reduce((sum, width) => sum + width, 0)
  const redistributedPanes = panes.map((pane) => {
    const width = pane.width / survivorWidthSum
    return { ...pane, width }
  })

  const activePaneIndex = indexAfterRemoval(group.activePane, paneIndex, panes.length)
  const updatedGroup: Group = { ...group, panes: redistributedPanes, activePane: activePaneIndex }

  return settled(withGroupAt(workspace, groupIndex, updatedGroup))
}

// A change that hands the pane back untouched leaves the workspace untouched too.
function withPaneChanged(
  workspace: Workspace,
  id: SessionId,
  change: (pane: Pane) => Pane
): Workspace {
  const location = locatePane(workspace, id)
  if (location === null) {
    return workspace
  }

  const { groupIndex, paneIndex } = location
  const group = workspace.groups[groupIndex]
  const pane = group.panes[paneIndex]
  const changed = change(pane)
  if (changed === pane) {
    return workspace
  }

  const panes = spliced(group.panes, paneIndex, 1, [changed])

  const updatedGroup: Group = { ...group, panes }
  return withGroupAt(workspace, groupIndex, updatedGroup)
}

export function renamePane(workspace: Workspace, id: SessionId, title: string): Workspace {
  return withPaneChanged(workspace, id, (pane) => {
    // A shell republishes its title on every prompt, so re-titling to the same
    // string would re-render the sidebar for nothing.
    if (pane.title === title) {
      return pane
    }
    return { ...pane, title }
  })
}

export function setPaneColor(
  workspace: Workspace,
  id: SessionId,
  color: RowColor | null
): Workspace {
  return withPaneChanged(workspace, id, (pane) => {
    if (pane.color === color) {
      return pane
    }
    return { ...pane, color }
  })
}

/** The workspace with the active pane in a color, or without it if the pane already had it. */
export function toggleActivePaneColor(workspace: Workspace, color: RowColor): Workspace {
  const pane = activePane(workspace)
  if (!pane) {
    return workspace
  }
  if (pane.color === color) {
    return setPaneColor(workspace, pane.id, null)
  }
  return setPaneColor(workspace, pane.id, color)
}

// Every operation keeps each pane at or above MIN_PANE_WIDTH, so a group holds
// at most ten panes and a group's widths can never sum to zero.
export function groupHasRoom(group: Group): boolean {
  const paneCount = group.panes.length + 1
  const shareEach = 1 / paneCount

  return shareEach >= MIN_PANE_WIDTH
}

export function canSplitPane(workspace: Workspace, donor: SessionId): boolean {
  const location = locatePane(workspace, donor)
  if (location === null) {
    return false
  }

  const group = workspace.groups[location.groupIndex]
  return groupHasRoom(group)
}

export function splitPane(workspace: Workspace, donor: SessionId, id: SessionId): Workspace {
  const location = locatePane(workspace, donor)
  if (location === null) {
    return workspace
  }

  const groupIndex = location.groupIndex
  const donorIndex = location.paneIndex
  const group = workspace.groups[groupIndex]
  if (!groupHasRoom(group)) {
    return workspace
  }

  const newPane: Pane = { id, title: '', width: 0, color: null }

  const arranged = spliced(group.panes, donorIndex + 1, 0, [newPane])

  // The whole group is levelled, not just the pane that was split. Giving the
  // new pane half of its donor would halve it again on the next split, so five
  // splits would leave the last pane a thirty-second of the width of the first.
  const shareEach = 1 / arranged.length
  const panes = arranged.map((pane) => {
    return { ...pane, width: shareEach }
  })

  const updatedGroup: Group = { ...group, panes, activePane: donorIndex + 1 }
  const replaced = withGroupAt(workspace, groupIndex, updatedGroup)

  const split: Workspace = { ...replaced, activeGroup: groupIndex }
  return openedAt(split, groupIndex)
}

export function forkPane(workspace: Workspace, source: SessionId, id: SessionId): Workspace {
  const panes = workspace.groups.flatMap((group) => group.panes)
  const sourcePane = panes.find((pane) => pane.id === source)
  if (!sourcePane) {
    return workspace
  }

  const split = splitPane(workspace, source, id)
  if (split === workspace) {
    return workspace
  }

  return setPaneColor(split, id, sourcePane.color)
}

export function activatePane(workspace: Workspace, id: SessionId): Workspace {
  const location = locatePane(workspace, id)
  if (location === null) {
    return workspace
  }

  const { groupIndex, paneIndex } = location
  const group = workspace.groups[groupIndex]
  const alreadyActive = groupIndex === workspace.activeGroup && paneIndex === group.activePane
  if (alreadyActive) {
    return workspace
  }

  const updatedGroup: Group = { ...group, activePane: paneIndex }
  const replaced = withGroupAt(workspace, groupIndex, updatedGroup)

  const activated: Workspace = { ...replaced, activeGroup: groupIndex }
  return openedAt(activated, groupIndex)
}

function focusPaneByDelta(workspace: Workspace, delta: number): Workspace {
  if (workspace.groups.length === 0) {
    return workspace
  }

  const group = workspace.groups[workspace.activeGroup]
  if (group.panes.length < 2) {
    return workspace
  }

  const paneCount = group.panes.length
  let nextIndex = group.activePane + delta
  if (nextIndex < 0) {
    nextIndex = paneCount - 1
  }
  if (nextIndex >= paneCount) {
    nextIndex = 0
  }

  const updatedGroup: Group = { ...group, activePane: nextIndex }
  return withGroupAt(workspace, workspace.activeGroup, updatedGroup)
}

export function focusNextPane(workspace: Workspace): Workspace {
  return focusPaneByDelta(workspace, 1)
}

export function focusPreviousPane(workspace: Workspace): Workspace {
  return focusPaneByDelta(workspace, -1)
}

function nextShowingGroup(workspace: Workspace, delta: number): number | null {
  const groupCount = workspace.groups.length
  for (let step = 1; step < groupCount; step += 1) {
    const stepped = workspace.activeGroup + delta * step
    const wrapped = ((stepped % groupCount) + groupCount) % groupCount
    if (!isHidden(workspace, wrapped)) {
      return wrapped
    }
  }
  return null
}

// Each group keeps its own activePane, so arriving back at a split group puts
// you on the pane you were last using in it.
function focusGroupByDelta(workspace: Workspace, delta: number): Workspace {
  if (workspace.groups.length < 2) {
    return workspace
  }

  const nextIndex = nextShowingGroup(workspace, delta)
  if (nextIndex === null) {
    return workspace
  }

  return { ...workspace, activeGroup: nextIndex }
}

export function focusNextGroup(workspace: Workspace): Workspace {
  return focusGroupByDelta(workspace, 1)
}

export function focusPreviousGroup(workspace: Workspace): Workspace {
  return focusGroupByDelta(workspace, -1)
}

export function moveDivider(
  workspace: Workspace,
  groupIndex: number,
  dividerIndex: number,
  leftWidth: number
): Workspace {
  const groupInRange = groupIndex >= 0 && groupIndex < workspace.groups.length
  if (!groupInRange) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const dividerInRange = dividerIndex >= 0 && dividerIndex < group.panes.length - 1
  if (!dividerInRange) {
    return workspace
  }

  const left = group.panes[dividerIndex]
  const right = group.panes[dividerIndex + 1]
  const pair = left.width + right.width
  const lowerBound = MIN_PANE_WIDTH
  const upperBound = pair - MIN_PANE_WIDTH
  if (lowerBound > upperBound) {
    return workspace
  }

  const boundedBelow = Math.max(lowerBound, leftWidth)
  const clampedLeftWidth = Math.min(upperBound, boundedBelow)
  if (clampedLeftWidth === left.width) {
    return workspace
  }

  const rightWidth = pair - clampedLeftWidth
  const updatedLeft: Pane = { ...left, width: clampedLeftWidth }
  const updatedRight: Pane = { ...right, width: rightWidth }

  const panes = spliced(group.panes, dividerIndex, 2, [updatedLeft, updatedRight])

  const updatedGroup: Group = { ...group, panes }
  return withGroupAt(workspace, groupIndex, updatedGroup)
}

export function paneOf(group: Group): Pane {
  return group.panes[group.activePane]
}

export function activePane(workspace: Workspace): Pane | null {
  if (workspace.groups.length === 0) {
    return null
  }

  const group = workspace.groups[workspace.activeGroup]
  return paneOf(group)
}
