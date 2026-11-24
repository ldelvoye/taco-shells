import { describe, expect, it } from 'vitest'

import { applyDrop } from './applyDrop'
import type { Dragged, Drop, Group, TabGroup, Workspace } from './model'

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

function makeWorkspace(groups: Group[], activeGroup: number, tabGroups: TabGroup[] = []): Workspace {
  return { groups, activeGroup, tabGroups }
}

function threeGroups(): Group[] {
  return [makeGroup('a', 'pane-a'), makeGroup('b', 'pane-b'), makeGroup('c', 'pane-c')]
}

describe('applyDrop', () => {
  it('moves a pane out of a split, into one, and within one', () => {
    const split = makeGroupWithWidths('s', [0.5, 0.25, 0.25], 1)
    const start = makeWorkspace([makeGroup('a', 'pane-a'), split], 0)

    const left = applyDrop(start, { kind: 'pane', pane: 's-0' }, { kind: 'between', index: 2, tabGroup: null }, 'new')
    expect(left.groups.map((group) => group.id)).toEqual(['a', 's', 'new'])
    expect(left.groups[1].panes.map((pane) => pane.width)).toEqual([0.5, 0.5])
    expect(left.groups[1].activePane).toBe(0)
    expect(left.groups[2].panes).toEqual([{ id: 's-0', title: '', width: 1, color: null }])
    expect(left.groups[2].activePane).toBe(0)
    expect(left.activeGroup).toBe(2)

    const joined = applyDrop(start, { kind: 'pane', pane: 'pane-a' }, { kind: 'into', group: 's', slot: 2 }, 'new')
    expect(joined.groups.map((group) => group.id)).toEqual(['s'])
    expect(joined.groups[0].panes.map((pane) => pane.id)).toEqual(['s-0', 's-1', 'pane-a', 's-2'])
    expect(joined.groups[0].panes.map((pane) => pane.width)).toEqual([0.25, 0.25, 0.25, 0.25])
    expect(joined.groups[0].activePane).toBe(2)
    expect(joined.activeGroup).toBe(0)

    const reordered = applyDrop(start, { kind: 'pane', pane: 's-0' }, { kind: 'into', group: 's', slot: 3 }, 'new')
    expect(reordered.groups[1].panes.map((pane) => pane.id)).toEqual(['s-1', 's-2', 's-0'])
    expect(reordered.groups[1].panes.map((pane) => pane.width)).toEqual([0.25, 0.25, 0.5])
    expect(reordered.groups[1].activePane).toBe(2)
    expect(reordered.activeGroup).toBe(1)

    const pair = makeWorkspace([makeGroupWithWidths('p', [0.7, 0.3], 0)], 0)
    const parted = applyDrop(pair, { kind: 'pane', pane: 'p-1' }, { kind: 'between', index: 0, tabGroup: null }, 'new')
    expect(parted.groups.map((group) => group.id)).toEqual(['new', 'p'])
    expect(parted.groups[1].panes.map((pane) => pane.width)).toEqual([1])
    expect(parted.activeGroup).toBe(0)
  })

  it('moves a terminal or a whole split to a gap, keeping its group', () => {
    const start = makeWorkspace(threeGroups(), 0)

    const terminal = applyDrop(start, { kind: 'pane', pane: 'pane-c' }, { kind: 'between', index: 0, tabGroup: null }, 'new')
    expect(terminal.groups.map((group) => group.id)).toEqual(['c', 'a', 'b'])
    expect(terminal.activeGroup).toBe(0)

    const whole = applyDrop(start, { kind: 'group', group: 'c' }, { kind: 'between', index: 0, tabGroup: null }, 'new')
    expect(whole.groups.map((group) => group.id)).toEqual(['c', 'a', 'b'])
    expect(whole.activeGroup).toBe(1)

    const down = applyDrop(start, { kind: 'group', group: 'a' }, { kind: 'between', index: 2, tabGroup: null }, 'new')
    expect(down.groups.map((group) => group.id)).toEqual(['b', 'a', 'c'])
  })

  it('keeps the same group active across a reorder', () => {
    const movingTheActiveGroup = makeWorkspace(threeGroups(), 0)
    const afterMovingActive = applyDrop(movingTheActiveGroup, { kind: 'group', group: 'a' }, { kind: 'between', index: 3, tabGroup: null }, 'new')
    expect(afterMovingActive.groups.map((group) => group.id)).toEqual(['b', 'c', 'a'])
    expect(afterMovingActive.activeGroup).toBe(2)

    const movingAnotherGroupPastTheActiveOne = makeWorkspace(threeGroups(), 1)
    const afterMovingOther = applyDrop(movingAnotherGroupPastTheActiveOne, { kind: 'group', group: 'a' }, { kind: 'between', index: 3, tabGroup: null }, 'new')
    expect(afterMovingOther.groups.map((group) => group.id)).toEqual(['b', 'c', 'a'])
    expect(afterMovingOther.activeGroup).toBe(0)
  })

  it('returns the same workspace reference for a drop that does nothing', () => {
    const tenWidths = Array.from({ length: 10 }, () => 0.1)
    const full = makeGroupWithWidths('full', tenWidths, 0)
    const pair = makeGroupWithWidths('s', [0.5, 0.5], 0)
    const start = makeWorkspace([makeGroup('a', 'pane-a'), pair, full], 0)

    const cases: [Dragged, Drop][] = [
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'between', index: 0, tabGroup: null }],
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'between', index: 1, tabGroup: null }],
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'into', group: 'a', slot: 1 }],
      [{ kind: 'pane', pane: 's-0' }, { kind: 'into', group: 's', slot: 0 }],
      [{ kind: 'pane', pane: 's-0' }, { kind: 'into', group: 's', slot: 1 }],
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'into', group: 'full', slot: 0 }],
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'into', group: 'gone', slot: 0 }],
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'into', group: 's', slot: 3 }],
      [{ kind: 'pane', pane: 's-0' }, { kind: 'between', index: 4, tabGroup: null }],
      [{ kind: 'group', group: 's' }, { kind: 'between', index: -1, tabGroup: null }],
      [{ kind: 'pane', pane: 'gone' }, { kind: 'between', index: 0, tabGroup: null }],
      [{ kind: 'group', group: 's' }, { kind: 'between', index: 1, tabGroup: null }],
      [{ kind: 'group', group: 's' }, { kind: 'between', index: 2, tabGroup: null }],
      [{ kind: 'group', group: 's' }, { kind: 'into', group: 'a', slot: 0 }],
      [{ kind: 'group', group: 'gone' }, { kind: 'between', index: 0, tabGroup: null }]
    ]

    for (const [dragged, drop] of cases) {
      const after = applyDrop(start, dragged, drop, 'new')
      expect(after).toBe(start)
    }
  })
})

function entry(id: string, tabGroup: string | null = null): Group {
  return {
    id,
    panes: [{ id: `pane-${id}`, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup
  }
}

function tab(id: string, collapsed = false): TabGroup {
  return { id, name: id, color: 'red', collapsed }
}

function layout(workspace: Workspace): string[] {
  return workspace.groups.map((group) => {
    let label = '-'
    if (group.tabGroup !== null) {
      label = group.tabGroup
    }
    return `${label}:${group.id}`
  })
}

describe('applyDrop with tab groups', () => {
  function start(): Workspace {
    const groups = [entry('a', 't1'), entry('b', 't1'), entry('c'), entry('d', 't2')]
    return makeWorkspace(groups, 2, [tab('t1'), tab('t2')])
  }

  it('moves entries into, out of and with a group, by the group the drop names', () => {
    const joined = applyDrop(start(), { kind: 'pane', pane: 'pane-c' }, { kind: 'between', index: 1, tabGroup: 't1' }, 'new')
    expect(layout(joined)).toEqual(['t1:a', 't1:c', 't1:b', 't2:d'])

    const left = applyDrop(start(), { kind: 'pane', pane: 'pane-b' }, { kind: 'between', index: 2, tabGroup: null }, 'new')
    expect(layout(left)).toEqual(['t1:a', '-:b', '-:c', 't2:d'])

    const leftInPlace = applyDrop(start(), { kind: 'pane', pane: 'pane-a' }, { kind: 'between', index: 0, tabGroup: null }, 'new')
    expect(layout(leftInPlace)).toEqual(['-:a', 't1:b', '-:c', 't2:d'])

    const emptied = applyDrop(start(), { kind: 'pane', pane: 'pane-d' }, { kind: 'between', index: 2, tabGroup: 't1' }, 'new')
    expect(layout(emptied)).toEqual(['t1:a', 't1:b', 't1:d', '-:c'])
    expect(emptied.tabGroups.map((group) => group.id)).toEqual(['t1'])

    const wholeSplit = applyDrop(start(), { kind: 'group', group: 'c' }, { kind: 'between', index: 0, tabGroup: 't1' }, 'new')
    expect(layout(wholeSplit)).toEqual(['t1:c', 't1:a', 't1:b', 't2:d'])

    const split = makeGroupWithWidths('s', [0.5, 0.5], 0)
    const withSplit = makeWorkspace([entry('a', 't1'), split], 1, [tab('t1')])
    const paneOut = applyDrop(withSplit, { kind: 'pane', pane: 's-1' }, { kind: 'between', index: 1, tabGroup: 't1' }, 'new')
    expect(layout(paneOut)).toEqual(['t1:a', 't1:new', '-:s'])

    const wholeGroup = applyDrop(start(), { kind: 'tabGroup', tabGroup: 't1' }, { kind: 'between', index: 4, tabGroup: null }, 'new')
    expect(layout(wholeGroup)).toEqual(['-:c', 't2:d', 't1:a', 't1:b'])
    expect(wholeGroup.activeGroup).toBe(0)
  })

  it('moves focus on when a dropped terminal lands in a collapsed group', () => {
    const groups = [entry('a'), entry('b', 't1'), entry('c')]
    const hidden = makeWorkspace(groups, 0, [tab('t1', true)])

    const dropped = applyDrop(hidden, { kind: 'pane', pane: 'pane-a' }, { kind: 'between', index: 2, tabGroup: 't1' }, 'new')

    expect(layout(dropped)).toEqual(['t1:b', 't1:a', '-:c'])
    expect(dropped.activeGroup).toBe(2)
  })

  it('refuses a drop that would split a group or change nothing', () => {
    const before = start()
    const cases: [Dragged, Drop][] = [
      [{ kind: 'pane', pane: 'pane-c' }, { kind: 'between', index: 1, tabGroup: null }],
      [{ kind: 'pane', pane: 'pane-c' }, { kind: 'between', index: 4, tabGroup: 't1' }],
      [{ kind: 'pane', pane: 'pane-c' }, { kind: 'between', index: 1, tabGroup: 'gone' }],
      [{ kind: 'pane', pane: 'pane-a' }, { kind: 'between', index: 0, tabGroup: 't1' }],
      [{ kind: 'tabGroup', tabGroup: 't2' }, { kind: 'between', index: 1, tabGroup: null }],
      [{ kind: 'tabGroup', tabGroup: 't1' }, { kind: 'between', index: 1, tabGroup: null }],
      [{ kind: 'tabGroup', tabGroup: 't1' }, { kind: 'between', index: 2, tabGroup: null }],
      [{ kind: 'tabGroup', tabGroup: 't1' }, { kind: 'between', index: 4, tabGroup: 't2' }],
      [{ kind: 'tabGroup', tabGroup: 't1' }, { kind: 'into', group: 'c', slot: 0 }],
      [{ kind: 'tabGroup', tabGroup: 'gone' }, { kind: 'between', index: 0, tabGroup: null }]
    ]

    for (const [dragged, drop] of cases) {
      const after = applyDrop(before, dragged, drop, 'new')
      expect(after).toBe(before)
    }
  })
})
