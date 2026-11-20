import type { Dragged, Drop, Group, Workspace } from './model'
import { applyDrop } from './model'

const PREVIEW_GROUP = 'preview'

export const EDGE_BAND = 0.25

/** The row under the pointer, and how far down that row the pointer is: 0 at its top, 1 at its bottom. */
export interface RowPoint {
  group: number
  pane: number
  fraction: number
}

/** The row at `offset` pixels down a group that is `height` pixels tall and holds `paneCount` equal rows. */
export function rowPointIn(
  group: number,
  paneCount: number,
  offset: number,
  height: number
): RowPoint {
  const rowHeight = height / paneCount
  const rowsDown = offset / rowHeight
  const aboveEnd = Math.min(rowsDown, paneCount)
  const withinGroup = Math.max(aboveEnd, 0)

  const lastPane = paneCount - 1
  const pane = Math.min(Math.floor(withinGroup), lastPane)

  const rawFraction = withinGroup - pane
  const aboveZero = Math.max(rawFraction, 0)
  const fraction = Math.min(aboveZero, 1)

  return { group, pane, fraction }
}

function candidateForGroup(group: Group, point: RowPoint): Drop {
  const rowsDown = point.pane + point.fraction
  const position = rowsDown / group.panes.length
  if (position < 0.5) {
    return { kind: 'between', index: point.group }
  }
  return { kind: 'between', index: point.group + 1 }
}

function candidateForPane(group: Group, point: RowPoint): Drop {
  const onFirstRow = point.pane === 0
  const onLastRow = point.pane === group.panes.length - 1

  if (onFirstRow && point.fraction < EDGE_BAND) {
    return { kind: 'between', index: point.group }
  }
  if (onLastRow && point.fraction > 1 - EDGE_BAND) {
    return { kind: 'between', index: point.group + 1 }
  }

  if (point.fraction < 0.5) {
    return { kind: 'into', group: group.id, slot: point.pane }
  }
  return { kind: 'into', group: group.id, slot: point.pane + 1 }
}

/** What dropping at this row would do, or null when it would do nothing. */
export function dropFor(workspace: Workspace, dragged: Dragged, point: RowPoint): Drop | null {
  const group = workspace.groups[point.group]
  if (!group) {
    return null
  }

  let candidate: Drop
  if (dragged.kind === 'group') {
    candidate = candidateForGroup(group, point)
  } else {
    candidate = candidateForPane(group, point)
  }

  const applied = applyDrop(workspace, dragged, candidate, PREVIEW_GROUP)
  if (applied === workspace) {
    return null
  }

  return candidate
}
