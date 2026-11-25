import type {
  Group,
  GroupId,
  RowColor,
  TabGroup,
  TabGroupId,
  Workspace
} from '@protocol/arrangement'
import { ROW_COLORS } from '@protocol/arrangement'

import { settled } from './arrangement'

export const NEW_TAB_GROUP_NAME = 'New group'

function firstUnusedColor(workspace: Workspace): RowColor {
  const usedColors = workspace.tabGroups.map((tabGroup) => tabGroup.color)
  const unused = ROW_COLORS.find((color) => !usedColors.includes(color))
  if (unused === undefined) {
    return ROW_COLORS[0]
  }
  return unused
}

// An entry that leaves a tab group to start its own lands right after it, so both stay in one piece.
function slotAfterOldTabGroup(
  remaining: Group[],
  oldTabGroup: TabGroupId | null,
  from: number
): number {
  if (oldTabGroup === null) {
    return from
  }

  const labels = remaining.map((group) => group.tabGroup)
  const lastRemaining = labels.lastIndexOf(oldTabGroup)
  if (lastRemaining === -1) {
    return from
  }
  return lastRemaining + 1
}

export function createTabGroup(workspace: Workspace, entry: GroupId, id: TabGroupId): Workspace {
  const from = workspace.groups.findIndex((group) => group.id === entry)
  if (from === -1) {
    return workspace
  }

  const activeGroupId = workspace.groups[workspace.activeGroup].id
  const lifted = workspace.groups[from]
  const relabelled: Group = { ...lifted, tabGroup: id }

  const groupsBefore = workspace.groups.slice(0, from)
  const groupsAfter = workspace.groups.slice(from + 1)
  const remaining = [...groupsBefore, ...groupsAfter]
  const slot = slotAfterOldTabGroup(remaining, lifted.tabGroup, from)

  const remainingBefore = remaining.slice(0, slot)
  const remainingAfter = remaining.slice(slot)
  const groups = [...remainingBefore, relabelled, ...remainingAfter]
  const activeGroup = groups.findIndex((group) => group.id === activeGroupId)

  const record: TabGroup = {
    id,
    name: NEW_TAB_GROUP_NAME,
    color: firstUnusedColor(workspace),
    collapsed: false
  }
  const tabGroups = [...workspace.tabGroups, record]

  return settled({ ...workspace, groups, activeGroup, tabGroups })
}

function withTabGroupChanged(
  workspace: Workspace,
  id: TabGroupId,
  change: (tabGroup: TabGroup) => TabGroup
): Workspace {
  const record = workspace.tabGroups.find((tabGroup) => tabGroup.id === id)
  if (record === undefined) {
    return workspace
  }

  const changed = change(record)
  if (changed === record) {
    return workspace
  }

  const tabGroups = workspace.tabGroups.map((tabGroup) => {
    if (tabGroup === record) {
      return changed
    }
    return tabGroup
  })

  return { ...workspace, tabGroups }
}

export function renameTabGroup(workspace: Workspace, id: TabGroupId, name: string): Workspace {
  return withTabGroupChanged(workspace, id, (tabGroup) => {
    if (tabGroup.name === name) {
      return tabGroup
    }
    return { ...tabGroup, name }
  })
}

export function recolorTabGroup(workspace: Workspace, id: TabGroupId, color: RowColor): Workspace {
  return withTabGroupChanged(workspace, id, (tabGroup) => {
    if (tabGroup.color === color) {
      return tabGroup
    }
    return { ...tabGroup, color }
  })
}

export function ungroupTabGroup(workspace: Workspace, id: TabGroupId): Workspace {
  const exists = workspace.tabGroups.some((tabGroup) => tabGroup.id === id)
  if (!exists) {
    return workspace
  }

  const groups = workspace.groups.map((group): Group => {
    if (group.tabGroup === id) {
      return { ...group, tabGroup: null }
    }
    return group
  })
  const tabGroups = workspace.tabGroups.filter((tabGroup) => tabGroup.id !== id)

  return settled({ ...workspace, groups, tabGroups })
}

export function toggleTabGroup(workspace: Workspace, id: TabGroupId): Workspace {
  const toggled = withTabGroupChanged(workspace, id, (tabGroup) => {
    return { ...tabGroup, collapsed: !tabGroup.collapsed }
  })
  return settled(toggled)
}
