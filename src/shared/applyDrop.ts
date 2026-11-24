import type { SessionId } from './ipc'
import {
  blockOf,
  closePane,
  groupHasRoom,
  locatePane,
  settled,
  spliced,
  withGroupAt,
  type Dragged,
  type Drop,
  type Group,
  type GroupId,
  type TabGroupId,
  type Workspace
} from './model'

type BetweenDrop = Extract<Drop, { kind: 'between' }>

function moveGroup(workspace: Workspace, from: number, to: number): Workspace {
  const lastIndex = workspace.groups.length - 1
  const fromInRange = from >= 0 && from <= lastIndex
  const toInRange = to >= 0 && to <= lastIndex
  if (!fromInRange || !toInRange || from === to) {
    return workspace
  }

  const activeGroupId = workspace.groups[workspace.activeGroup].id

  const withoutMoved = spliced(workspace.groups, from, 1, [])

  const moved = workspace.groups[from]
  const groups = spliced(withoutMoved, to, 0, [moved])

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

  const labelled: Group = { ...entry, tabGroup }
  return withGroupAt(workspace, index, labelled)
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
  const rest = spliced(workspace.groups, block.first, blockSize, [])

  let to = drop.index
  if (drop.index > block.last) {
    to = drop.index - blockSize
  }

  const groups = spliced(rest, to, 0, entries)
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
  const groups = spliced(closed.groups, drop.index, 0, [alone])

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
  const withoutMoved = spliced(group.panes, paneIndex, 1, [])
  const panes = spliced(withoutMoved, to, 0, [pane])

  const updatedGroup: Group = { ...group, panes, activePane: to }
  const replaced = withGroupAt(workspace, groupIndex, updatedGroup)

  return { ...replaced, activeGroup: groupIndex }
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
  const arranged = spliced(target.panes, slot, 0, [pane])

  const shareEach = 1 / arranged.length
  const panes = arranged.map((arrangedPane) => {
    return { ...arrangedPane, width: shareEach }
  })

  const joined: Group = { ...target, panes, activePane: slot }
  const replaced = withGroupAt(closed, closedTargetIndex, joined)

  return { ...replaced, activeGroup: closedTargetIndex }
}

function dropPane(
  workspace: Workspace,
  id: SessionId,
  drop: Drop,
  newGroup: GroupId
): Workspace {
  const location = locatePane(workspace, id)
  if (location === null) {
    return workspace
  }

  const { groupIndex, paneIndex } = location
  const group = workspace.groups[groupIndex]

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
