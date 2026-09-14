import { describe, expect, it } from 'vitest'

import type { Group, Workspace } from './model'
import {
  activatePane,
  canSplitActivePane,
  closePane,
  emptyWorkspace,
  focusNextGroup,
  focusNextPane,
  focusPreviousGroup,
  focusPreviousPane,
  MIN_PANE_WIDTH,
  moveDivider,
  moveGroup,
  renamePane,
  splitActivePane
} from './model'

function makeGroup(id: string, paneId: string): Group {
  return { id, panes: [{ id: paneId, title: paneId, width: 1 }], activePane: 0 }
}

function makeGroupWithWidths(id: string, widths: number[], activePane: number): Group {
  const panes = widths.map((width, index) => ({ id: `${id}-${index}`, title: '', width }))
  return { id, panes, activePane }
}

function makeWorkspace(groups: Group[], activeGroup: number): Workspace {
  return { groups, activeGroup }
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

describe('splitActivePane', () => {
  it('puts the new pane after the active one and levels the whole group', () => {
    const group = makeGroupWithWidths('a', [0.5, 0.5], 0)
    const workspace = makeWorkspace([group], 0)

    const result = splitActivePane(workspace, 'new-pane')

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

describe('the minimum pane width', () => {
  it('holds across repeated splitting, which is what bounds a group', () => {
    let workspace = makeWorkspace([makeGroup('a', 'pane-a')], 0)

    let attempts = 0
    while (canSplitActivePane(workspace) && attempts < 100) {
      workspace = splitActivePane(workspace, `pane-${attempts}`)
      attempts += 1
    }

    const panes = workspace.groups[0].panes
    const narrowest = Math.min(...panes.map((pane) => pane.width))
    expect(narrowest).toBeGreaterThanOrEqual(MIN_PANE_WIDTH)

    // Widths summing to 1 with none below a tenth leaves room for ten panes and
    // no more, so the group cannot grow without bound.
    expect(panes).toHaveLength(10)
    expect(splitActivePane(workspace, 'one-too-many')).toBe(workspace)
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

describe('unchanged operations', () => {
  it('returns the same workspace reference when nothing changes', () => {
    const workspace = makeWorkspace([makeGroup('a', 'pane-a'), makeGroup('b', 'pane-b')], 0)

    expect(closePane(workspace, 'missing-session')).toBe(workspace)
    expect(renamePane(workspace, 'missing-session', 'new title')).toBe(workspace)
    expect(renamePane(workspace, 'pane-a', 'pane-a')).toBe(workspace)
    expect(moveGroup(workspace, 0, 0)).toBe(workspace)

    const empty = emptyWorkspace()
    expect(splitActivePane(empty, 'new-pane')).toBe(empty)
    expect(activatePane(workspace, 'pane-a')).toBe(workspace)
    expect(focusNextPane(workspace)).toBe(workspace)

    const loneGroup = makeWorkspace([makeGroup('a', 'pane-a')], 0)
    expect(focusNextGroup(loneGroup)).toBe(loneGroup)
    expect(moveDivider(workspace, 0, 5, 0.5)).toBe(workspace)
  })
})
