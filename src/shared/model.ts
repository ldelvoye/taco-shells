import type { SessionId } from './ipc'

export type GroupId = string

export interface Pane {
  id: SessionId
  title: string
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

  const activePaneIndex = indexAfterRemoval(group.activePane, paneIndex, panes.length)
  const updatedGroup: Group = { ...group, panes, activePane: activePaneIndex }
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

export function activateGroup(workspace: Workspace, index: number): Workspace {
  const inRange = index >= 0 && index < workspace.groups.length
  if (!inRange || index === workspace.activeGroup) {
    return workspace
  }

  return { groups: workspace.groups, activeGroup: index }
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
