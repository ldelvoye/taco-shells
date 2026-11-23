import { describe, expect, it } from 'vitest'

import type { Dragged, Drop, Group, TabGroup, Workspace } from './model'
import {
  activatePane,
  applyDrop,
  canSplitPane,
  closePane,
  emptyWorkspace,
  focusNextGroup,
  focusNextPane,
  focusPreviousGroup,
  focusPreviousPane,
  forkPane,
  MIN_PANE_WIDTH,
  moveDivider,
  moveGroup,
  renamePane,
  setPaneColor,
  splitPane,
  toggleActivePaneColor
} from './model'

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

describe('closePane', () => {
  it('keeps the active group sensible after a close', () => {
    const beforeActive = makeWorkspace(threeGroups(), 2)
    const afterClosingBeforeActive = closePane(beforeActive, 'pane-a')
    expect(afterClosingBeforeActive.groups.map((group) => group.id)).toEqual(['b', 'c'])
    expect(afterClosingBeforeActive.activeGroup).toBe(1)

    const activeMiddle = makeWorkspace(threeGroups(), 1)
    const afterClosingActiveMiddle = closePane(activeMiddle, 'pane-b')
    expect(afterClosingActiveMiddle.groups.map((group) => group.id)).toEqual(['a', 'c'])
    expect(afterClosingActiveMiddle.activeGroup).toBe(1)

    const activeBottom = makeWorkspace(threeGroups(), 2)
    const afterClosingActiveBottom = closePane(activeBottom, 'pane-c')
    expect(afterClosingActiveBottom.groups.map((group) => group.id)).toEqual(['a', 'b'])
    expect(afterClosingActiveBottom.activeGroup).toBe(1)
  })

  it('leaves an empty workspace with activeGroup back at 0 when the only group is closed', () => {
    const workspace = makeWorkspace([makeGroup('a', 'pane-a')], 0)

    const result = closePane(workspace, 'pane-a')

    expect(result.groups).toEqual([])
    expect(result.activeGroup).toBe(0)
  })
})

describe('moveGroup', () => {
  it('keeps the same group active across a reorder', () => {
    const movingTheActiveGroup = makeWorkspace(threeGroups(), 0)
    const afterMovingActive = moveGroup(movingTheActiveGroup, 0, 2)
    expect(afterMovingActive.groups.map((group) => group.id)).toEqual(['b', 'c', 'a'])
    expect(afterMovingActive.activeGroup).toBe(2)

    const movingAnotherGroupPastTheActiveOne = makeWorkspace(threeGroups(), 1)
    const afterMovingOther = moveGroup(movingAnotherGroupPastTheActiveOne, 0, 2)
    expect(afterMovingOther.groups.map((group) => group.id)).toEqual(['b', 'c', 'a'])
    expect(afterMovingOther.activeGroup).toBe(0)
  })
})

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

describe('splitPane', () => {
  it('puts the new pane after its donor and levels the whole group', () => {
    const group = makeGroupWithWidths('a', [0.5, 0.5], 1)
    const workspace = makeWorkspace([group], 0)

    const result = splitPane(workspace, 'a-0', 'new-pane')

    const resultGroup = result.groups[0]
    expect(resultGroup.panes.map((pane) => pane.id)).toEqual(['a-0', 'new-pane', 'a-1'])
    expect(resultGroup.activePane).toBe(1)

    // Every pane the same width, including the ones that were not split: the
    // alternative halves the newest pane each time and fans the group out.
    const widths = resultGroup.panes.map((pane) => pane.width)
    expect(widths[0]).toBe(widths[1])
    expect(widths[1]).toBe(widths[2])
  })
})

describe('forkPane', () => {
  it('puts the fork right of its source in any group, with its color, and focuses it', () => {
    const colored: Group = {
      id: 'g0',
      panes: [
        { id: 'a', title: '', width: 0.5, color: 'blue' },
        { id: 'b', title: '', width: 0.5, color: null }
      ],
      activePane: 1,
      tabGroup: null
    }
    const other = makeGroup('g1', 'c')
    const workspace = makeWorkspace([colored, other], 1)

    const forkOfColored = forkPane(workspace, 'a', 'f')

    const forkedGroup = forkOfColored.groups[0]
    expect(forkedGroup.panes.map((pane) => pane.id)).toEqual(['a', 'f', 'b'])
    for (const pane of forkedGroup.panes) {
      expect(pane.width).toBeCloseTo(1 / 3)
    }
    expect(forkedGroup.panes[1].color).toBe('blue')
    expect(forkedGroup.activePane).toBe(1)
    expect(forkOfColored.activeGroup).toBe(0)
    expect(forkOfColored.groups[1]).toBe(other)

    const forkOfPlain = forkPane(workspace, 'b', 'f')
    expect(forkOfPlain.groups[0].panes[2].color).toBeNull()

    expect(forkPane(workspace, 'missing', 'f')).toBe(workspace)

    const fullGroup = makeGroupWithWidths('full', new Array(10).fill(0.1), 0)
    const fullWorkspace = makeWorkspace([fullGroup], 0)
    expect(forkPane(fullWorkspace, 'full-3', 'f')).toBe(fullWorkspace)
  })
})

describe('the minimum pane width', () => {
  it('holds across repeated splitting, which is what bounds a group', () => {
    let workspace = makeWorkspace([makeGroup('a', 'pane-a')], 0)

    let attempts = 0
    while (canSplitPane(workspace, 'pane-a') && attempts < 100) {
      workspace = splitPane(workspace, 'pane-a', `pane-${attempts}`)
      attempts += 1
    }

    const panes = workspace.groups[0].panes
    const narrowest = Math.min(...panes.map((pane) => pane.width))
    expect(narrowest).toBeGreaterThanOrEqual(MIN_PANE_WIDTH)

    // Widths summing to 1 with none below a tenth leaves room for ten panes and
    // no more, so the group cannot grow without bound.
    expect(panes).toHaveLength(10)
    expect(splitPane(workspace, 'pane-a', 'one-too-many')).toBe(workspace)
  })
})

describe('closePane width redistribution', () => {
  it('redistributes the closed pane width proportionally across survivors', () => {
    const group = makeGroupWithWidths('a', [0.5, 0.25, 0.25], 2)
    const workspace = makeWorkspace([group], 0)

    const result = closePane(workspace, 'a-0')

    const resultGroup = result.groups[0]
    expect(resultGroup.panes.map((pane) => pane.width)).toEqual([0.5, 0.5])
    expect(resultGroup.activePane).toBe(1)
  })
})

describe('focus wrapping', () => {
  it('wraps activePane at both ends of the group', () => {
    const lastActive = makeGroupWithWidths('a', [0.5, 0.25, 0.25], 2)
    const next = focusNextPane(makeWorkspace([lastActive], 0))
    expect(next.groups[0].activePane).toBe(0)

    const firstActive = makeGroupWithWidths('a', [0.5, 0.25, 0.25], 0)
    const previous = focusPreviousPane(makeWorkspace([firstActive], 0))
    expect(previous.groups[0].activePane).toBe(2)
  })
})

describe('group focus', () => {
  it('wraps at both ends and leaves each group on the pane it was last using', () => {
    const split = makeGroupWithWidths('b', [0.5, 0.5], 1)
    const groups = [makeGroup('a', 'pane-a'), split, makeGroup('c', 'pane-c')]

    const fromLast = focusNextGroup(makeWorkspace(groups, 2))
    expect(fromLast.activeGroup).toBe(0)

    const fromFirst = focusPreviousGroup(makeWorkspace(groups, 0))
    expect(fromFirst.activeGroup).toBe(2)

    const ontoTheSplit = focusNextGroup(makeWorkspace(groups, 0))
    expect(ontoTheSplit.activeGroup).toBe(1)
    expect(ontoTheSplit.groups[1].activePane).toBe(1)
  })
})

describe('moveDivider', () => {
  it('adjusts adjacent panes while preserving their combined width, and clamps at the floor', () => {
    const group = makeGroupWithWidths('a', [0.25, 0.25, 0.5], 0)
    const workspace = makeWorkspace([group], 0)

    const moved = moveDivider(workspace, 0, 0, 0.375)
    const movedPanes = moved.groups[0].panes
    expect(movedPanes[0].width).toBe(0.375)
    expect(movedPanes[1].width).toBe(0.125)
    expect(movedPanes[2].width).toBe(0.5)

    const clamped = moveDivider(workspace, 0, 0, 0.01)
    expect(clamped.groups[0].panes[0].width).toBe(MIN_PANE_WIDTH)
  })
})

describe('activatePane', () => {
  it('reaches a pane in another group, and a pane beside it in the same one', () => {
    const groups = [makeGroup('a', 'pane-a'), makeGroupWithWidths('b', [0.5, 0.5], 0)]
    const workspace = makeWorkspace(groups, 0)

    const acrossGroups = activatePane(workspace, 'b-1')
    expect(acrossGroups.activeGroup).toBe(1)
    expect(acrossGroups.groups[1].activePane).toBe(1)

    const withinGroup = activatePane(acrossGroups, 'b-0')
    expect(withinGroup.activeGroup).toBe(1)
    expect(withinGroup.groups[1].activePane).toBe(0)
  })
})

describe('toggleActivePaneColor', () => {
  it('colors only the active pane, swaps a different color, and clears the same one', () => {
    const start = makeWorkspace([makeGroupWithWidths('a', [0.5, 0.5], 1)], 0)
    const colorsOf = (workspace: Workspace): Array<string | null> => {
      return workspace.groups[0].panes.map((pane) => pane.color)
    }

    const blue = toggleActivePaneColor(start, 'blue')
    expect(colorsOf(blue)).toEqual([null, 'blue'])

    const red = toggleActivePaneColor(blue, 'red')
    expect(colorsOf(red)).toEqual([null, 'red'])

    const cleared = toggleActivePaneColor(red, 'red')
    expect(colorsOf(cleared)).toEqual([null, null])
  })
})

describe('unchanged operations', () => {
  it('returns the same workspace reference when nothing changes', () => {
    const workspace = makeWorkspace([makeGroup('a', 'pane-a'), makeGroup('b', 'pane-b')], 0)

    expect(closePane(workspace, 'missing-session')).toBe(workspace)
    expect(renamePane(workspace, 'missing-session', 'new title')).toBe(workspace)
    expect(renamePane(workspace, 'pane-a', 'pane-a')).toBe(workspace)
    expect(setPaneColor(workspace, 'pane-a', null)).toBe(workspace)
    expect(moveGroup(workspace, 0, 0)).toBe(workspace)

    const empty = emptyWorkspace()
    expect(splitPane(empty, 'pane-a', 'new-pane')).toBe(empty)
    expect(activatePane(workspace, 'pane-a')).toBe(workspace)
    expect(focusNextPane(workspace)).toBe(workspace)

    const loneGroup = makeWorkspace([makeGroup('a', 'pane-a')], 0)
    expect(focusNextGroup(loneGroup)).toBe(loneGroup)
    expect(moveDivider(workspace, 0, 5, 0.5)).toBe(workspace)
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
