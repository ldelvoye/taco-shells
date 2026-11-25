import type { Group, Pane, TabGroupId, Workspace } from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'

export const MIN_PANE_WIDTH = 0.1

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

/** The id of the terminal that has the keyboard, or null before there is one. */
export function activeSessionOf(workspace: Workspace | null): SessionId | null {
  if (workspace === null) {
    return null
  }

  const pane = activePane(workspace)
  if (pane === null) {
    return null
  }

  return pane.id
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

/** The workspace with the group at this index replaced. */
export function withGroupAt(workspace: Workspace, index: number, group: Group): Workspace {
  const groups = spliced(workspace.groups, index, 1, [group])
  return { ...workspace, groups }
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
