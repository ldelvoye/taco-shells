import { describe, expect, it } from 'vitest'

import type { DropPoint } from './drop'
import { dropFor, rowPointIn } from './drop'
import type { Dragged, Drop, Group, TabGroup, Workspace } from '@protocol/arrangement'

function makeGroup(id: string, paneId: string): Group {
  return {
    id,
    panes: [{ id: paneId, title: paneId, width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function makeGroupWithWidths(id: string, widths: number[], activePane: number): Group {
  const panes = widths.map((width, index) => {
    return { id: `${id}-${index}`, title: '', width, color: null }
  })
  return { id, panes, activePane, tabGroup: null }
}

function makeWorkspace(groups: Group[], activeGroup: number): Workspace {
  return { groups, activeGroup, tabGroups: [] }
}

describe('dropFor', () => {
  it('reads the bands of the row under the pointer when one terminal is dragged', () => {
    const split = makeGroupWithWidths('s', [0.4, 0.3, 0.3], 0)
    const groups = [makeGroup('a', 'pane-a'), split, makeGroup('c', 'pane-c')]
    const workspace = makeWorkspace(groups, 0)
    const dragged: Dragged = { kind: 'pane', pane: 'pane-c' }

    const cases: [string, DropPoint, Drop | null][] = [
      ['top edge of a terminal', { over: 'row', group: 0, pane: 0, fraction: 0.2 }, { kind: 'between', index: 0, tabGroup: null }],
      ['upper middle of a terminal', { over: 'row', group: 0, pane: 0, fraction: 0.3 }, { kind: 'into', group: 'a', slot: 0 }],
      ['lower middle of a terminal', { over: 'row', group: 0, pane: 0, fraction: 0.7 }, { kind: 'into', group: 'a', slot: 1 }],
      ['bottom edge of a terminal', { over: 'row', group: 0, pane: 0, fraction: 0.8 }, { kind: 'between', index: 1, tabGroup: null }],
      ['top edge of a split', { over: 'row', group: 1, pane: 0, fraction: 0.2 }, { kind: 'between', index: 1, tabGroup: null }],
      ['bottom of a first row', { over: 'row', group: 1, pane: 0, fraction: 0.8 }, { kind: 'into', group: 's', slot: 1 }],
      ['top of a middle row', { over: 'row', group: 1, pane: 1, fraction: 0.1 }, { kind: 'into', group: 's', slot: 1 }],
      ['lower middle of a last row', { over: 'row', group: 1, pane: 2, fraction: 0.6 }, { kind: 'into', group: 's', slot: 3 }],
      ['the gap it already sits in', { over: 'row', group: 1, pane: 2, fraction: 0.8 }, null],
      ['its own row', { over: 'row', group: 2, pane: 0, fraction: 0.5 }, null]
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

    const cases: [string, DropPoint, Drop | null][] = [
      ['upper half of a terminal', { over: 'row', group: 0, pane: 0, fraction: 0.4 }, { kind: 'between', index: 0, tabGroup: null }],
      ['upper half of a split', { over: 'row', group: 3, pane: 0, fraction: 0.9 }, { kind: 'between', index: 3, tabGroup: null }],
      ['lower half of a split', { over: 'row', group: 3, pane: 1, fraction: 0.1 }, { kind: 'between', index: 4, tabGroup: null }],
      ['the gap it already sits in', { over: 'row', group: 2, pane: 0, fraction: 0.2 }, null]
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

    const drop = dropFor(workspace, { kind: 'pane', pane: 'pane-a' }, { over: 'row', group: 1, pane: 4, fraction: 0.5 })

    expect(drop).toBeNull()
  })
})

describe('dropFor with tab groups', () => {
  function grouped(): Workspace {
    const split = makeGroupWithWidths('s', [0.5, 0.5], 0)
    const groups: Group[] = [
      { ...makeGroup('a', 'pane-a'), tabGroup: 't1' },
      { ...split, tabGroup: 't1' },
      makeGroup('c', 'pane-c'),
      { ...makeGroup('d', 'pane-d'), tabGroup: 't2' }
    ]
    const tabGroups: TabGroup[] = [
      { id: 't1', name: 't1', color: 'red', collapsed: false },
      { id: 't2', name: 't2', color: 'blue', collapsed: true }
    ]
    return { groups, activeGroup: 2, tabGroups }
  }

  it('lets the row or header under the pointer decide the group of one dragged terminal', () => {
    const dragged: Dragged = { kind: 'pane', pane: 'pane-c' }
    const cases: [string, DropPoint, Drop | null][] = [
      ['top edge of an open header', { over: 'header', tabGroup: 't1', fraction: 0.1 }, { kind: 'between', index: 0, tabGroup: null }],
      ['rest of an open header', { over: 'header', tabGroup: 't1', fraction: 0.5 }, { kind: 'between', index: 0, tabGroup: 't1' }],
      ['top edge of a collapsed header, where it already is', { over: 'header', tabGroup: 't2', fraction: 0.1 }, null],
      ['middle of a collapsed header', { over: 'header', tabGroup: 't2', fraction: 0.5 }, { kind: 'between', index: 4, tabGroup: 't2' }],
      ['bottom edge of a collapsed header', { over: 'header', tabGroup: 't2', fraction: 0.9 }, { kind: 'between', index: 4, tabGroup: null }],
      ['top edge of a member', { over: 'row', group: 0, pane: 0, fraction: 0.1 }, { kind: 'between', index: 0, tabGroup: 't1' }],
      ['bottom edge of the last member, where it already is', { over: 'row', group: 1, pane: 1, fraction: 0.9 }, { kind: 'between', index: 2, tabGroup: 't1' }],
      ['under the last row', { over: 'end' }, { kind: 'between', index: 4, tabGroup: null }]
    ]

    for (const [label, point, expected] of cases) {
      const drop = dropFor(grouped(), dragged, point)
      expect(drop, label).toEqual(expected)
    }
  })

  it('drops a whole group above or below the whole group or entry under the pointer', () => {
    const cases: [string, Dragged, DropPoint, Drop | null][] = [
      ['upper half of a collapsed header', { kind: 'tabGroup', tabGroup: 't1' }, { over: 'header', tabGroup: 't2', fraction: 0.3 }, { kind: 'between', index: 3, tabGroup: null }],
      ['lower half of a collapsed header', { kind: 'tabGroup', tabGroup: 't1' }, { over: 'header', tabGroup: 't2', fraction: 0.7 }, { kind: 'between', index: 4, tabGroup: null }],
      ['upper half of the entry right below it', { kind: 'tabGroup', tabGroup: 't1' }, { over: 'row', group: 2, pane: 0, fraction: 0.3 }, null],
      ['its own rows', { kind: 'tabGroup', tabGroup: 't1' }, { over: 'row', group: 1, pane: 0, fraction: 0.5 }, null],
      ['upper half of an open group, on a member', { kind: 'tabGroup', tabGroup: 't2' }, { over: 'row', group: 0, pane: 0, fraction: 0.5 }, { kind: 'between', index: 0, tabGroup: null }],
      ['lower half of an open group, on a member', { kind: 'tabGroup', tabGroup: 't2' }, { over: 'row', group: 1, pane: 0, fraction: 0.5 }, { kind: 'between', index: 2, tabGroup: null }],
      ['under the last row, where it already is', { kind: 'tabGroup', tabGroup: 't2' }, { over: 'end' }, null],
      ['a whole entry on the lower half of a member', { kind: 'group', group: 'c' }, { over: 'row', group: 0, pane: 0, fraction: 0.7 }, { kind: 'between', index: 1, tabGroup: 't1' }]
    ]

    for (const [label, dragged, point, expected] of cases) {
      const drop = dropFor(grouped(), dragged, point)
      expect(drop, label).toEqual(expected)
    }
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
