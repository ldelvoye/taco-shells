import type { SessionId } from './ipc'

export const MIN_PANE_WIDTH = 0.1

export type GroupId = string

export interface Pane {
  id: SessionId
  title: string
  /** Share of the group's width. A group's widths sum to 1. */
  width: number
}

export interface Group {
  id: GroupId
  panes: Pane[]
  activePane: number
}

export interface Workspace {
  groups: Group[]
  activeGroup: number
}

export function emptyWorkspace(): Workspace {
  return { groups: [], activeGroup: 0 }
}

export function addGroup(workspace: Workspace, group: Group): Workspace {
  const groups = [...workspace.groups, group]
  const activeGroup = groups.length - 1
  return { groups, activeGroup }
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

    return { groups, activeGroup }
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

  return { groups, activeGroup: workspace.activeGroup }
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

  return { groups, activeGroup }
}

export function renamePane(workspace: Workspace, id: SessionId, title: string): Workspace {
  const groupIndex = workspace.groups.findIndex((group) =>
    group.panes.some((pane) => pane.id === id)
  )
  if (groupIndex === -1) {
    return workspace
  }

  const group = workspace.groups[groupIndex]
  const paneIndex = group.panes.findIndex((pane) => pane.id === id)
  const pane = group.panes[paneIndex]

  // A shell republishes its title on every prompt, so re-titling to the same
  // string would re-render the sidebar for nothing.
  if (pane.title === title) {
    return workspace
  }

  const panes = [...group.panes]
  panes[paneIndex] = { ...pane, title }

  const updatedGroup: Group = { ...group, panes }
  const groups = [...workspace.groups]
  groups[groupIndex] = updatedGroup

  return { groups, activeGroup: workspace.activeGroup }
}

// Every operation keeps each pane at or above MIN_PANE_WIDTH, so a group holds
// at most ten panes and a group's widths can never sum to zero.
export function canSplitActivePane(workspace: Workspace): boolean {
  if (workspace.groups.length === 0) {
    return false
  }

  const group = workspace.groups[workspace.activeGroup]
  const paneCount = group.panes.length + 1
  const shareEach = 1 / paneCount

  return shareEach >= MIN_PANE_WIDTH
}

export function splitActivePane(workspace: Workspace, id: SessionId): Workspace {
  if (!canSplitActivePane(workspace)) {
    return workspace
  }

  const group = workspace.groups[workspace.activeGroup]
  const donorIndex = group.activePane
  const newPane: Pane = { id, title: '', width: 0 }

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
  groups[workspace.activeGroup] = updatedGroup

  return { groups, activeGroup: workspace.activeGroup }
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

  return { groups, activeGroup: groupIndex }
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

  return { groups, activeGroup: workspace.activeGroup }
}

export function focusNextPane(workspace: Workspace): Workspace {
  return focusPaneByDelta(workspace, 1)
}

export function focusPreviousPane(workspace: Workspace): Workspace {
  return focusPaneByDelta(workspace, -1)
}

// Each group keeps its own activePane, so arriving back at a split group puts
// you on the pane you were last using in it.
function focusGroupByDelta(workspace: Workspace, delta: number): Workspace {
  if (workspace.groups.length < 2) {
    return workspace
  }

  const groupCount = workspace.groups.length
  let nextIndex = workspace.activeGroup + delta
  if (nextIndex < 0) {
    nextIndex = groupCount - 1
  }
  if (nextIndex >= groupCount) {
    nextIndex = 0
  }

  return { groups: workspace.groups, activeGroup: nextIndex }
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

  return { groups, activeGroup: workspace.activeGroup }
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
