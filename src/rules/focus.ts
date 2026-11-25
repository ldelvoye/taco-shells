import type { Group, Workspace } from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'
import type { FocusMove } from '@protocol/workspace'

import { isHidden, locatePane, openedAt, withGroupAt } from './arrangement'

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

/** The arrangement after one focus move; the same object when the move goes nowhere. */
export function focusAfter(workspace: Workspace, move: FocusMove): Workspace {
  switch (move) {
    case 'nextPane':
      return focusNextPane(workspace)
    case 'previousPane':
      return focusPreviousPane(workspace)
    case 'nextGroup':
      return focusNextGroup(workspace)
    case 'previousGroup':
      return focusPreviousGroup(workspace)
  }
}
