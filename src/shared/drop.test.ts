import { describe, expect, it } from 'vitest'

import type { RowPoint } from './drop'
import { dropFor, rowPointIn } from './drop'
import type { Dragged, Drop, Group, Workspace } from './model'

function makeGroup(id: string, paneId: string): Group {
  return { id, panes: [{ id: paneId, title: paneId, width: 1, color: null }], activePane: 0 }
}

function makeGroupWithWidths(id: string, widths: number[], activePane: number): Group {
  const panes = widths.map((width, index) => {
    return { id: `${id}-${index}`, title: '', width, color: null }
  })
  return { id, panes, activePane }
}

function makeWorkspace(groups: Group[], activeGroup: number): Workspace {
  return { groups, activeGroup }
}

describe('dropFor', () => {
  it('reads the bands of the row under the pointer when one terminal is dragged', () => {
    const split = makeGroupWithWidths('s', [0.4, 0.3, 0.3], 0)
    const groups = [makeGroup('a', 'pane-a'), split, makeGroup('c', 'pane-c')]
    const workspace = makeWorkspace(groups, 0)
    const dragged: Dragged = { kind: 'pane', pane: 'pane-c' }

    const cases: [string, RowPoint, Drop | null][] = [
      ['top edge of a terminal', { group: 0, pane: 0, fraction: 0.2 }, { kind: 'between', index: 0 }],
      ['upper middle of a terminal', { group: 0, pane: 0, fraction: 0.3 }, { kind: 'into', group: 'a', slot: 0 }],
      ['lower middle of a terminal', { group: 0, pane: 0, fraction: 0.7 }, { kind: 'into', group: 'a', slot: 1 }],
      ['bottom edge of a terminal', { group: 0, pane: 0, fraction: 0.8 }, { kind: 'between', index: 1 }],
      ['top edge of a split', { group: 1, pane: 0, fraction: 0.2 }, { kind: 'between', index: 1 }],
      ['bottom of a first row', { group: 1, pane: 0, fraction: 0.8 }, { kind: 'into', group: 's', slot: 1 }],
      ['top of a middle row', { group: 1, pane: 1, fraction: 0.1 }, { kind: 'into', group: 's', slot: 1 }],
      ['lower middle of a last row', { group: 1, pane: 2, fraction: 0.6 }, { kind: 'into', group: 's', slot: 3 }],
      ['the gap it already sits in', { group: 1, pane: 2, fraction: 0.8 }, null],
      ['its own row', { group: 2, pane: 0, fraction: 0.5 }, null]
    ]

    for (const [label, point, expected] of cases) {
      const drop = dropFor(workspace, dragged, point)
      expect(drop, label).toEqual(expected)
    }
  })

  it('drops a whole split by the half of the terminal or split under the pointer', () => {
    const split = makeGroupWithWidths('s', [0.4, 0.3, 0.3], 0)
    const pair = makeGroupWithWidths('d', [0.5, 0.5], 0)
    const groups = [makeGroup('a', 'pane-a'), split, makeGroup('c', 'pane-c'), pair]
    const workspace = makeWorkspace(groups, 0)
    const dragged: Dragged = { kind: 'group', group: 's' }

    const cases: [string, RowPoint, Drop | null][] = [
      ['upper half of a terminal', { group: 0, pane: 0, fraction: 0.4 }, { kind: 'between', index: 0 }],
      ['upper half of a split', { group: 3, pane: 0, fraction: 0.9 }, { kind: 'between', index: 3 }],
      ['lower half of a split', { group: 3, pane: 1, fraction: 0.1 }, { kind: 'between', index: 4 }],
      ['the gap it already sits in', { group: 2, pane: 0, fraction: 0.2 }, null]
    ]

    for (const [label, point, expected] of cases) {
      const drop = dropFor(workspace, dragged, point)
      expect(drop, label).toEqual(expected)
    }
  })

  it('offers nothing over a split that is full', () => {
    const tenWidths = Array.from({ length: 10 }, () => 0.1)
    const full = makeGroupWithWidths('full', tenWidths, 0)
    const workspace = makeWorkspace([makeGroup('a', 'pane-a'), full], 0)

    const drop = dropFor(workspace, { kind: 'pane', pane: 'pane-a' }, { group: 1, pane: 4, fraction: 0.5 })

    expect(drop).toBeNull()
  })
})

describe('rowPointIn', () => {
  it('finds the row and stays inside the group at its edges', () => {
    const middle = rowPointIn(1, 3, 30, 66)
    expect(middle.group).toBe(1)
    expect(middle.pane).toBe(1)
    expect(middle.fraction).toBeCloseTo(8 / 22)

    expect(rowPointIn(1, 3, 66, 66)).toEqual({ group: 1, pane: 2, fraction: 1 })
    expect(rowPointIn(1, 3, -4, 66)).toEqual({ group: 1, pane: 0, fraction: 0 })
  })
})
