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

type BetweenDrop = Extract<Drop, { kind: 'between' }>

export function emptyWorkspace(): Workspace {
  return { groups: [], activeGroup: 0, tabGroups: [] }
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
  let index = workspace.groups.length
  if (anchor !== undefined && anchor.tabGroup !== null) {
    const block = blockOf(workspace, anchor.tabGroup)
    if (block !== null) {
      tabGroup = anchor.tabGroup
      index = block.last + 1
    }
  }

  const entry: Group = { ...group, tabGroup }
  const groupsBefore = workspace.groups.slice(0, index)
  const groupsAfter = workspace.groups.slice(index)
  const groups = [...groupsBefore, entry, ...groupsAfter]

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
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === id)
  )
  if (groupIndex === -1) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const paneIndex = group.panes.findIndex((pane) => pane.id === id)
  const panesBefore = group.panes.slice(0, paneIndex)
  const panesAfter = group.panes.slice(paneIndex + 1)
  const panes = [...panesBefore, ...panesAfter]

  if (panes.length === 0) {
    const groupsBefore = workspace.groups.slice(0, groupIndex)
    const groupsAfter = workspace.groups.slice(groupIndex + 1)
    const groups = [...groupsBefore, ...groupsAfter]
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
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  return settled({ ...workspace, groups })
}

export function moveGroup(workspace: Workspace, from: number, to: number): Workspace {
  const lastIndex = workspace.groups.length - 1
  const fromInRange = from >= 0 && from <= lastIndex
  const toInRange = to >= 0 && to <= lastIndex
  if (!fromInRange || !toInRange || from === to) {
    return workspace
  }

  const activeGroupId = workspace.groups[workspace.activeGroup].id

  const groupsBefore = workspace.groups.slice(0, from)
  const groupsAfter = workspace.groups.slice(from + 1)
  const withoutMoved = [...groupsBefore, ...groupsAfter]

  const moved = workspace.groups[from]
  const before = withoutMoved.slice(0, to)
  const after = withoutMoved.slice(to)
  const groups = [...before, moved, ...after]

  const activeGroup = groups.findIndex((group) => group.id === activeGroupId)

  return { ...workspace, groups, activeGroup }
}

function gapInRange(gap: number, length: number): boolean {
  return gap >= 0 && gap <= length
}

function indexAfterLift(gap: number, lifted: number): number {
  if (gap > lifted) {
    return gap - 1
  }
  return gap
}

function gapInsideATabGroup(workspace: Workspace, gap: number): boolean {
  return workspace.tabGroups.some((tabGroup) => {
    const block = blockOf(workspace, tabGroup.id)
    if (block === null) {
      return false
    }
    return block.first < gap && gap <= block.last
  })
}

function betweenIsAllowed(workspace: Workspace, drop: BetweenDrop): boolean {
  if (!gapInRange(drop.index, workspace.groups.length)) {
    return false
  }
  if (drop.tabGroup === null) {
    return !gapInsideATabGroup(workspace, drop.index)
  }

  const block = blockOf(workspace, drop.tabGroup)
  if (block === null) {
    return false
  }
  return drop.index >= block.first && drop.index <= block.last + 1
}

function withEntryLabelled(
  workspace: Workspace,
  index: number,
  tabGroup: TabGroupId | null
): Workspace {
  const entry = workspace.groups[index]
  if (entry.tabGroup === tabGroup) {
    return workspace
  }

  const groups = [...workspace.groups]
  groups[index] = { ...entry, tabGroup }

  return { ...workspace, groups }
}

function moveEntryToGap(
  workspace: Workspace,
  from: number,
  gap: number,
  tabGroup: TabGroupId | null
): Workspace {
  const to = indexAfterLift(gap, from)
  const moved = moveGroup(workspace, from, to)
  return withEntryLabelled(moved, to, tabGroup)
}

function dropGroup(workspace: Workspace, id: GroupId, drop: Drop): Workspace {
  if (drop.kind !== 'between') {
    return workspace
  }

  const from = workspace.groups.findIndex((group) => group.id === id)
  if (from === -1) {
    return workspace
  }

  return moveEntryToGap(workspace, from, drop.index, drop.tabGroup)
}

function dropTabGroup(workspace: Workspace, id: TabGroupId, drop: Drop): Workspace {
  if (drop.kind !== 'between' || drop.tabGroup !== null) {
    return workspace
  }

  const block = blockOf(workspace, id)
  if (block === null) {
    return workspace
  }

  const onItsOwnBlockOrEdge = drop.index >= block.first && drop.index <= block.last + 1
  if (onItsOwnBlockOrEdge) {
    return workspace
  }

  const activeGroupId = workspace.groups[workspace.activeGroup].id
  const blockSize = block.last - block.first + 1
  const entries = workspace.groups.slice(block.first, block.last + 1)
  const groupsBefore = workspace.groups.slice(0, block.first)
  const groupsAfter = workspace.groups.slice(block.last + 1)
  const rest = [...groupsBefore, ...groupsAfter]

  let to = drop.index
  if (drop.index > block.last) {
    to = drop.index - blockSize
  }

  const restBefore = rest.slice(0, to)
  const restAfter = rest.slice(to)
  const groups = [...restBefore, ...entries, ...restAfter]
  const activeGroup = groups.findIndex((group) => group.id === activeGroupId)

  return { ...workspace, groups, activeGroup }
}

function moveLoneGroupToGap(
  workspace: Workspace,
  from: number,
  gap: number,
  tabGroup: TabGroupId | null
): Workspace {
  const moved = moveEntryToGap(workspace, from, gap, tabGroup)
  if (moved === workspace) {
    return workspace
  }

  const movedId = workspace.groups[from].id
  const activeGroup = moved.groups.findIndex((group) => group.id === movedId)

  return { ...moved, activeGroup }
}

function leaveToGap(
  workspace: Workspace,
  groupIndex: number,
  paneIndex: number,
  drop: BetweenDrop,
  newGroup: GroupId
): Workspace {
  const pane = workspace.groups[groupIndex].panes[paneIndex]
  const closed = closePane(workspace, pane.id)

  const alone: Group = {
    id: newGroup,
    panes: [{ ...pane, width: 1 }],
    activePane: 0,
    tabGroup: drop.tabGroup
  }
  const groupsBefore = closed.groups.slice(0, drop.index)
  const groupsAfter = closed.groups.slice(drop.index)
  const groups = [...groupsBefore, alone, ...groupsAfter]

  const activeGroup = groups.findIndex((group) => group.id === newGroup)

  return { ...closed, groups, activeGroup }
}

function reorderWithin(
  workspace: Workspace,
  groupIndex: number,
  paneIndex: number,
  slot: number
): Workspace {
  const to = indexAfterLift(slot, paneIndex)
  if (to === paneIndex) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const pane = group.panes[paneIndex]
  const panesBefore = group.panes.slice(0, paneIndex)
  const panesAfter = group.panes.slice(paneIndex + 1)
  const withoutMoved = [...panesBefore, ...panesAfter]

  const before = withoutMoved.slice(0, to)
  const after = withoutMoved.slice(to)
  const panes = [...before, pane, ...after]

  const updatedGroup: Group = { ...group, panes, activePane: to }
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  return { ...workspace, groups, activeGroup: groupIndex }
}

function joinGroup(
  workspace: Workspace,
  groupIndex: number,
  paneIndex: number,
  targetIndex: number,
  slot: number
): Workspace {
  const pane = workspace.groups[groupIndex].panes[paneIndex]
  const targetId = workspace.groups[targetIndex].id
  const closed = closePane(workspace, pane.id)

  const closedTargetIndex = closed.groups.findIndex((group) => group.id === targetId)
  const target = closed.groups[closedTargetIndex]
  const panesBefore = target.panes.slice(0, slot)
  const panesAfter = target.panes.slice(slot)
  const arranged = [...panesBefore, pane, ...panesAfter]

  const shareEach = 1 / arranged.length
  const panes = arranged.map((arrangedPane) => {
    return { ...arrangedPane, width: shareEach }
  })

  const joined: Group = { ...target, panes, activePane: slot }
  const groups = [...closed.groups]
  groups[closedTargetIndex] = joined

  return { ...closed, groups, activeGroup: closedTargetIndex }
}

function dropPane(
  workspace: Workspace,
  id: SessionId,
  drop: Drop,
  newGroup: GroupId
): Workspace {
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === id)
  )
  if (groupIndex === -1) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const paneIndex = group.panes.findIndex((pane) => pane.id === id)

  if (drop.kind === 'between') {
    if (group.panes.length === 1) {
      return moveLoneGroupToGap(workspace, groupIndex, drop.index, drop.tabGroup)
    }
    return leaveToGap(workspace, groupIndex, paneIndex, drop, newGroup)
  }

  const targetIndex = workspace.groups.findIndex((candidate) => candidate.id === drop.group)
  if (targetIndex === -1) {
    return workspace
  }

  const target = workspace.groups[targetIndex]
  if (!gapInRange(drop.slot, target.panes.length)) {
    return workspace
  }
  if (targetIndex === groupIndex) {
    return reorderWithin(workspace, groupIndex, paneIndex, drop.slot)
  }
  if (!groupHasRoom(target)) {
    return workspace
  }
  return joinGroup(workspace, groupIndex, paneIndex, targetIndex, drop.slot)
}

function dropDragged(
  workspace: Workspace,
  dragged: Dragged,
  drop: Drop,
  newGroup: GroupId
): Workspace {
  if (dragged.kind === 'tabGroup') {
    return dropTabGroup(workspace, dragged.tabGroup, drop)
  }
  if (dragged.kind === 'group') {
    return dropGroup(workspace, dragged.group, drop)
  }

  return dropPane(workspace, dragged.pane, drop, newGroup)
}

export function applyDrop(
  workspace: Workspace,
  dragged: Dragged,
  drop: Drop,
  newGroup: GroupId
): Workspace {
  if (drop.kind === 'between' && !betweenIsAllowed(workspace, drop)) {
    return workspace
  }

  const dropped = dropDragged(workspace, dragged, drop, newGroup)
  if (dropped === workspace) {
    return workspace
  }

  return settled(dropped)
}

// A change that hands the pane back untouched leaves the workspace untouched too.
function withPaneChanged(
  workspace: Workspace,
  id: SessionId,
  change: (pane: Pane) => Pane
): Workspace {
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === id)
  )
  if (groupIndex === -1) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const paneIndex = group.panes.findIndex((pane) => pane.id === id)
  const pane = group.panes[paneIndex]
  const changed = change(pane)
  if (changed === pane) {
    return workspace
  }

  const panes = [...group.panes]
  panes[paneIndex] = changed

  const updatedGroup: Group = { ...group, panes }
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  return { ...workspace, groups }
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
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === donor)
  )
  if (groupIndex === -1) {
    return false
  }

  const group = workspace.groups[groupIndex]
  return groupHasRoom(group)
}

export function splitPane(workspace: Workspace, donor: SessionId, id: SessionId): Workspace {
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === donor)
  )
  if (groupIndex === -1) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  if (!groupHasRoom(group)) {
    return workspace
  }

  const donorIndex = group.panes.findIndex((pane) => pane.id === donor)
  const newPane: Pane = { id, title: '', width: 0, color: null }

  const panesBefore = group.panes.slice(0, donorIndex + 1)
  const panesAfter = group.panes.slice(donorIndex + 1)
  const arranged = [...panesBefore, newPane, ...panesAfter]

  // The whole group is levelled, not just the pane that was split. Giving the
  // new pane half of its donor would halve it again on the next split, so five
  // splits would leave the last pane a thirty-second of the width of the first.
  const shareEach = 1 / arranged.length
  const panes = arranged.map((pane) => {
    return { ...pane, width: shareEach }
  })

  const updatedGroup: Group = { ...group, panes, activePane: donorIndex + 1 }
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  const split: Workspace = { ...workspace, groups, activeGroup: groupIndex }
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

export function canSplitActivePane(workspace: Workspace): boolean {
  const pane = activePane(workspace)
  if (!pane) {
    return false
  }

  return canSplitPane(workspace, pane.id)
}

export function splitActivePane(workspace: Workspace, id: SessionId): Workspace {
  const pane = activePane(workspace)
  if (!pane) {
    return workspace
  }

  return splitPane(workspace, pane.id, id)
}

export function activatePane(workspace: Workspace, id: SessionId): Workspace {
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === id)
  )
  if (groupIndex === -1) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const paneIndex = group.panes.findIndex((pane) => pane.id === id)
  const alreadyActive = groupIndex === workspace.activeGroup && paneIndex === group.activePane
  if (alreadyActive) {
    return workspace
  }

  const updatedGroup: Group = { ...group, activePane: paneIndex }
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  const activated: Workspace = { ...workspace, groups, activeGroup: groupIndex }
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
  const groups = [...workspace.groups]
  groups[workspace.activeGroup] = updatedGroup

  return { ...workspace, groups }
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

  const panesBefore = group.panes.slice(0, dividerIndex)
  const panesAfter = group.panes.slice(dividerIndex + 2)
  const panes = [...panesBefore, updatedLeft, updatedRight, ...panesAfter]

  const updatedGroup: Group = { ...group, panes }
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  return { ...workspace, groups }
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
