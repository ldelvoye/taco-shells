import type { Dragged, Drop, Group, TabGroup, TabGroupId, Workspace } from './model'
import { applyDrop, blockOf } from './model'

// Names the group a previewed drop would create; the preview is thrown away, so it never reaches the screen.
const PREVIEW_GROUP = 'preview'

export const EDGE_BAND = 0.25

/** The row under the pointer, and how far down that row the pointer is: 0 at its top, 1 at its bottom. */
export interface RowPoint {
  group: number
  pane: number
  fraction: number
}

export type DropPoint =
  | ({ over: 'row' } & RowPoint)
  | { over: 'header'; tabGroup: TabGroupId; fraction: number }
  | { over: 'end' }

/** The row at `offset` pixels down a group that is `height` pixels tall and holds `paneCount` equal rows. */
export function rowPointIn(
  group: number,
  paneCount: number,
  offset: number,
  height: number
): RowPoint {
  const rowHeight = height / paneCount
  const rowsDown = offset / rowHeight
  const row = Math.floor(rowsDown)

  const lastPane = paneCount - 1
  const pane = clamp(row, 0, lastPane)
  const fraction = clamp(rowsDown - pane, 0, 1)

  return { group, pane, fraction }
}

function clamp(value: number, lowest: number, highest: number): number {
  const raised = Math.max(value, lowest)
  return Math.min(raised, highest)
}

function candidateForGroup(group: Group, point: RowPoint): Drop {
  const rowsDown = point.pane + point.fraction
  const inUpperHalf = rowsDown / group.panes.length < 0.5
  if (inUpperHalf) {
    return { kind: 'between', index: point.group, tabGroup: group.tabGroup }
  }
  return { kind: 'between', index: point.group + 1, tabGroup: group.tabGroup }
}

function candidateForPane(group: Group, point: RowPoint): Drop {
  const onFirstRow = point.pane === 0
  const onLastRow = point.pane === group.panes.length - 1

  if (onFirstRow && point.fraction < EDGE_BAND) {
    return { kind: 'between', index: point.group, tabGroup: group.tabGroup }
  }
  if (onLastRow && point.fraction > 1 - EDGE_BAND) {
    return { kind: 'between', index: point.group + 1, tabGroup: group.tabGroup }
  }

  if (point.fraction < 0.5) {
    return { kind: 'into', group: group.id, slot: point.pane }
  }
  return { kind: 'into', group: group.id, slot: point.pane + 1 }
}

function candidateForHeader(
  workspace: Workspace,
  tabGroupId: TabGroupId,
  fraction: number
): Drop | null {
  const record = workspace.tabGroups.find((tabGroup) => tabGroup.id === tabGroupId)
  const block = blockOf(workspace, tabGroupId)
  if (record === undefined || block === null) {
    return null
  }

  if (fraction < EDGE_BAND) {
    return { kind: 'between', index: block.first, tabGroup: null }
  }
  if (!record.collapsed) {
    return { kind: 'between', index: block.first, tabGroup: tabGroupId }
  }
  if (fraction > 1 - EDGE_BAND) {
    return { kind: 'between', index: block.last + 1, tabGroup: null }
  }
  return { kind: 'between', index: block.last + 1, tabGroup: tabGroupId }
}

function candidateForTerminal(
  workspace: Workspace,
  dragged: Dragged,
  point: DropPoint
): Drop | null {
  if (point.over === 'end') {
    return { kind: 'between', index: workspace.groups.length, tabGroup: null }
  }
  if (point.over === 'header') {
    return candidateForHeader(workspace, point.tabGroup, point.fraction)
  }

  const group = workspace.groups[point.group]
  if (!group) {
    return null
  }
  if (dragged.kind === 'group') {
    return candidateForGroup(group, point)
  }
  return candidateForPane(group, point)
}

function paneRowsIn(groups: Group[]): number {
  return groups.reduce((sum, group) => sum + group.panes.length, 0)
}

function linesOf(workspace: Workspace, tabGroup: TabGroup, first: number, last: number): number {
  if (tabGroup.collapsed) {
    return 1
  }

  const entries = workspace.groups.slice(first, last + 1)
  return 1 + paneRowsIn(entries)
}

function aboveOrBelowTabGroup(
  workspace: Workspace,
  tabGroupId: TabGroupId,
  line: number,
  fraction: number
): Drop | null {
  const record = workspace.tabGroups.find((tabGroup) => tabGroup.id === tabGroupId)
  const block = blockOf(workspace, tabGroupId)
  if (record === undefined || block === null) {
    return null
  }

  const lineCount = linesOf(workspace, record, block.first, block.last)
  const position = (line + fraction) / lineCount
  if (position < 0.5) {
    return { kind: 'between', index: block.first, tabGroup: null }
  }
  return { kind: 'between', index: block.last + 1, tabGroup: null }
}

function candidateForTabGroup(workspace: Workspace, point: DropPoint): Drop | null {
  if (point.over === 'end') {
    return { kind: 'between', index: workspace.groups.length, tabGroup: null }
  }
  if (point.over === 'header') {
    return aboveOrBelowTabGroup(workspace, point.tabGroup, 0, point.fraction)
  }

  const group = workspace.groups[point.group]
  if (!group) {
    return null
  }
  if (group.tabGroup === null) {
    return candidateForGroup(group, point)
  }

  const block = blockOf(workspace, group.tabGroup)
  if (block === null) {
    return null
  }
  const entriesBefore = workspace.groups.slice(block.first, point.group)
  const line = 1 + paneRowsIn(entriesBefore) + point.pane
  return aboveOrBelowTabGroup(workspace, group.tabGroup, line, point.fraction)
}

function candidateFor(workspace: Workspace, dragged: Dragged, point: DropPoint): Drop | null {
  if (dragged.kind === 'tabGroup') {
    return candidateForTabGroup(workspace, point)
  }
  return candidateForTerminal(workspace, dragged, point)
}

/** What dropping at this point would do, or null when it would do nothing. */
export function dropFor(workspace: Workspace, dragged: Dragged, point: DropPoint): Drop | null {
  const candidate = candidateFor(workspace, dragged, point)
  if (candidate === null) {
    return null
  }

  // applyDrop hands back the same workspace for a drop that changes nothing or is refused.
  const applied = applyDrop(workspace, dragged, candidate, PREVIEW_GROUP)
  if (applied === workspace) {
    return null
  }

  return candidate
}
