import type { Group, Pane, RowColor, TabGroupId, Workspace } from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'

import {
  MIN_PANE_WIDTH,
  activePane,
  blockOf,
  groupHasRoom,
  isHidden,
  locatePane,
  openedAt,
  spliced,
  withGroupAt
} from '../arrangement'

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
