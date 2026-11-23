import { describe, expect, it } from 'vitest'

import type { Group, TabGroup, Workspace } from './model'
import {
  activatePane,
  addGroup,
  addGroupTo,
  closePane,
  focusNextGroup,
  focusPreviousGroup,
  settled,
  splitActivePane
} from './model'
import {
  createTabGroup,
  recolorTabGroup,
  renameTabGroup,
  toggleTabGroup,
  ungroupTabGroup
} from './tabGroups'

function entry(id: string, tabGroup: string | null = null): Group {
  return { id, panes: [{ id: `pane-${id}`, title: '', width: 1, color: null }], activePane: 0, tabGroup }
}

function tab(id: string, collapsed = false): TabGroup {
  return { id, name: id, color: 'red', collapsed }
}

function workspaceOf(groups: Group[], activeGroup: number, tabGroups: TabGroup[] = []): Workspace {
  return { groups, activeGroup, tabGroups }
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

describe('createTabGroup', () => {
  it('starts a group from an entry, taking it out of the group it was in', () => {
    const start = workspaceOf([entry('a', 't1'), entry('b', 't1'), entry('c')], 0, [tab('t1')])

    const fromLoose = createTabGroup(start, 'c', 'new')
    expect(layout(fromLoose)).toEqual(['t1:a', 't1:b', 'new:c'])
    expect(fromLoose.tabGroups[1]).toEqual({ id: 'new', name: 'New group', color: 'orange', collapsed: false })

    const fromGrouped = createTabGroup(start, 'a', 'new')
    expect(layout(fromGrouped)).toEqual(['t1:b', 'new:a', '-:c'])
    expect(fromGrouped.activeGroup).toBe(1)

    const lastOfItsGroup = workspaceOf([entry('a', 't1'), entry('c')], 0, [tab('t1')])
    const replaced = createTabGroup(lastOfItsGroup, 'a', 'new')
    expect(layout(replaced)).toEqual(['new:a', '-:c'])
    expect(replaced.tabGroups.map((group) => group.id)).toEqual(['new'])

    const allLoose = workspaceOf([entry('a'), entry('b'), entry('c')], 0)
    expect(layout(createTabGroup(allLoose, 'b', 'new'))).toEqual(['-:a', 'new:b', '-:c'])
  })
})

describe('toggleTabGroup', () => {
  it('moves focus out of a group that collapses, to the nearest entry still showing', () => {
    const start = workspaceOf([entry('x'), entry('a', 't1'), entry('b', 't1'), entry('y')], 1, [tab('t1')])
    const collapsed = toggleTabGroup(start, 't1')
    expect(collapsed.tabGroups[0].collapsed).toBe(true)
    expect(collapsed.activeGroup).toBe(3)

    const reopened = toggleTabGroup(collapsed, 't1')
    expect(reopened.tabGroups[0].collapsed).toBe(false)
    expect(reopened.activeGroup).toBe(3)

    const nothingBelow = workspaceOf([entry('x'), entry('a', 't1')], 1, [tab('t1')])
    expect(toggleTabGroup(nothingBelow, 't1').activeGroup).toBe(0)

    const pastAHiddenOne = workspaceOf(
      [entry('a', 't1'), entry('b', 't2'), entry('c')],
      0,
      [tab('t1'), tab('t2', true)]
    )
    expect(toggleTabGroup(pastAHiddenOne, 't1').activeGroup).toBe(2)

    const nowhereToGo = workspaceOf([entry('a', 't1')], 0, [tab('t1')])
    const stillThere = toggleTabGroup(nowhereToGo, 't1')
    expect(stillThere.tabGroups[0].collapsed).toBe(true)
    expect(stillThere.activeGroup).toBe(0)
  })
})

describe('addGroupTo', () => {
  it('adds to the end of the named group, or ungrouped at the bottom', () => {
    const fromLoose = workspaceOf([entry('a', 't1'), entry('b', 't1'), entry('c')], 2, [tab('t1')])
    const intoGroup = addGroupTo(fromLoose, entry('n'), 't1')
    expect(layout(intoGroup)).toEqual(['t1:a', 't1:b', 't1:n', '-:c'])
    expect(intoGroup.activeGroup).toBe(2)

    const fromGrouped = workspaceOf([entry('a', 't1'), entry('c')], 0, [tab('t1')])
    const ungrouped = addGroupTo(fromGrouped, entry('n'), null)
    expect(layout(ungrouped)).toEqual(['t1:a', '-:c', '-:n'])
    expect(ungrouped.activeGroup).toBe(2)

    const collapsed = workspaceOf([entry('a', 't1'), entry('c')], 1, [tab('t1', true)])
    const opened = addGroupTo(collapsed, entry('n'), 't1')
    expect(layout(opened)).toEqual(['t1:a', 't1:n', '-:c'])
    expect(opened.tabGroups[0].collapsed).toBe(false)
    expect(opened.activeGroup).toBe(1)

    const gone = addGroupTo(fromGrouped, entry('n'), 'gone')
    expect(layout(gone)).toEqual(['t1:a', '-:c', '-:n'])
  })
})

describe('a tab group as terminals close, open and take focus', () => {
  it('keeps its invariants through the existing operations', () => {
    const lastMember = workspaceOf([entry('a', 't1'), entry('b')], 1, [tab('t1')])
    expect(closePane(lastMember, 'pane-a').tabGroups).toEqual([])

    const hidden = workspaceOf([entry('a'), entry('b', 't1'), entry('c')], 0, [tab('t1', true)])
    const closed = closePane(hidden, 'pane-a')
    expect(layout(closed)).toEqual(['t1:b', '-:c'])
    expect(closed.activeGroup).toBe(1)

    const inAGroup = workspaceOf([entry('a', 't1'), entry('b', 't1'), entry('c')], 0, [tab('t1')])
    const joined = addGroup(inAGroup, entry('n'))
    expect(layout(joined)).toEqual(['t1:a', 't1:b', 't1:n', '-:c'])
    expect(joined.activeGroup).toBe(2)

    const outside = workspaceOf([entry('a', 't1'), entry('c')], 1, [tab('t1')])
    expect(layout(addGroup(outside, entry('n')))).toEqual(['t1:a', '-:c', '-:n'])

    const allHidden = workspaceOf([entry('a', 't1')], 0, [tab('t1', true)])
    const opened = addGroup(allHidden, entry('n'))
    expect(layout(opened)).toEqual(['t1:a', 't1:n'])
    expect(opened.tabGroups[0].collapsed).toBe(false)

    const splitWhileHidden = splitActivePane(allHidden, 'pane-new')
    expect(splitWhileHidden.tabGroups[0].collapsed).toBe(false)

    const jumped = activatePane(hidden, 'pane-b')
    expect(jumped.tabGroups[0].collapsed).toBe(false)
    expect(jumped.activeGroup).toBe(1)

    expect(focusNextGroup(hidden).activeGroup).toBe(2)
    expect(focusPreviousGroup(hidden).activeGroup).toBe(2)
  })
})

describe('editing a tab group', () => {
  it('renames, recolors and ungroups, and hands back the same workspace when nothing changes', () => {
    const start = workspaceOf([entry('a', 't1'), entry('b', 't1'), entry('c')], 0, [tab('t1')])

    expect(renameTabGroup(start, 't1', 'pebbles').tabGroups[0].name).toBe('pebbles')
    expect(recolorTabGroup(start, 't1', 'blue').tabGroups[0].color).toBe('blue')

    const loose = ungroupTabGroup(start, 't1')
    expect(layout(loose)).toEqual(['-:a', '-:b', '-:c'])
    expect(loose.tabGroups).toEqual([])

    const allHidden = workspaceOf(
      [entry('a', 't1'), entry('b', 't2')],
      0,
      [tab('t1', true), tab('t2', true)]
    )
    expect(ungroupTabGroup(allHidden, 't2').activeGroup).toBe(1)

    expect(renameTabGroup(start, 't1', 't1')).toBe(start)
    expect(recolorTabGroup(start, 't1', 'red')).toBe(start)
    expect(renameTabGroup(start, 'gone', 'x')).toBe(start)
    expect(recolorTabGroup(start, 'gone', 'blue')).toBe(start)
    expect(ungroupTabGroup(start, 'gone')).toBe(start)
    expect(toggleTabGroup(start, 'gone')).toBe(start)
    expect(createTabGroup(start, 'gone', 'new')).toBe(start)
    expect(settled(start)).toBe(start)
  })
})
