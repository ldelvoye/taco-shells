import { describe, expect, it } from 'vitest'

import type { Group, Workspace } from './model'
import { activateGroup, closePane, moveGroup, renamePane } from './model'

function makeGroup(id: string, paneId: string): Group {
  return { id, panes: [{ id: paneId, title: paneId }], activePane: 0 }
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

describe('unchanged operations', () => {
  it('returns the same workspace reference when nothing changes', () => {
    const workspace = makeWorkspace([makeGroup('a', 'pane-a'), makeGroup('b', 'pane-b')], 0)

    expect(closePane(workspace, 'missing-session')).toBe(workspace)
    expect(renamePane(workspace, 'missing-session', 'new title')).toBe(workspace)
    expect(renamePane(workspace, 'pane-a', 'pane-a')).toBe(workspace)
    expect(activateGroup(workspace, 0)).toBe(workspace)
    expect(moveGroup(workspace, 0, 0)).toBe(workspace)
  })
})
