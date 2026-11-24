import { describe, expect, it } from 'vitest'

import { type DropMark, dropMarkFor, sameDrop } from './dropMark'
import type { Drop, Group, TabGroup, Workspace } from './model'

function makeGroup(id: string, tabGroup: string | null): Group {
  return {
    id,
    panes: [{ id: `pane-${id}`, title: id, width: 1, color: null }],
    activePane: 0,
    tabGroup
  }
}

function makeTabGroup(id: string, collapsed: boolean): TabGroup {
  return { id, name: id, color: 'blue', collapsed }
}

function makeWorkspace(groups: Group[], tabGroups: TabGroup[]): Workspace {
  return { groups, activeGroup: 0, tabGroups }
}

function between(index: number, tabGroup: string | null): Drop {
  return { kind: 'between', index, tabGroup }
}

describe('dropMarkFor', () => {
  it('marks a drop outside any tab group above the entry or wrapper it lands before', () => {
    const groups = [makeGroup('a', null), makeGroup('b', 't'), makeGroup('c', 't'), makeGroup('d', null)]
    const workspace = makeWorkspace(groups, [makeTabGroup('t', false)])

    const cases: [string, number, DropMark][] = [
      ['a plain entry', 0, { target: { on: 'entry', index: 0 }, edge: 'above', tabGroup: null }],
      ['the first entry of a tab group', 1, { target: { on: 'wrapper', tabGroup: 't' }, edge: 'above', tabGroup: null }],
      ['the end after an ungrouped entry', 4, { target: { on: 'entry', index: 3 }, edge: 'below', tabGroup: null }]
    ]

    for (const [label, index, expected] of cases) {
      const mark = dropMarkFor(workspace, between(index, null))
      expect(mark, label).toEqual(expected)
    }
  })

  it('marks the end of a list that finishes in a tab group below the wrapper, and an empty list not at all', () => {
    const groups = [makeGroup('a', null), makeGroup('b', 't'), makeGroup('c', 't')]
    const endsInTabGroup = makeWorkspace(groups, [makeTabGroup('t', false)])
    const empty = makeWorkspace([], [])

    const wrapperMark = dropMarkFor(endsInTabGroup, between(3, null))
    const emptyMark = dropMarkFor(empty, between(0, null))

    expect(wrapperMark).toEqual({ target: { on: 'wrapper', tabGroup: 't' }, edge: 'below', tabGroup: null })
    expect(emptyMark).toBeNull()
  })

  it('marks a drop inside a tab group by where it falls in the block', () => {
    const groups = [makeGroup('a', null), makeGroup('b', 't'), makeGroup('c', 't'), makeGroup('d', null)]
    const open = makeWorkspace(groups, [makeTabGroup('t', false)])
    const collapsed = makeWorkspace(groups, [makeTabGroup('t', true)])

    const cases: [string, Workspace, number, DropMark][] = [
      ['a collapsed group', collapsed, 2, { target: { on: 'header', tabGroup: 't' }, edge: 'whole', tabGroup: 't' }],
      ['the block start', open, 1, { target: { on: 'header', tabGroup: 't' }, edge: 'below', tabGroup: 't' }],
      ['past the block end', open, 3, { target: { on: 'entry', index: 2 }, edge: 'below', tabGroup: 't' }],
      ['the middle of the block', open, 2, { target: { on: 'entry', index: 2 }, edge: 'above', tabGroup: 't' }]
    ]

    for (const [label, workspace, index, expected] of cases) {
      const mark = dropMarkFor(workspace, between(index, 't'))
      expect(mark, label).toEqual(expected)
    }

    const missing = dropMarkFor(open, between(1, 'gone'))
    expect(missing).toBeNull()
  })

  it('draws no line for a null drop or an into drop', () => {
    const workspace = makeWorkspace([makeGroup('a', null)], [])
    const into: Drop = { kind: 'into', group: 'a', slot: 0 }

    expect(dropMarkFor(workspace, null)).toBeNull()
    expect(dropMarkFor(workspace, into)).toBeNull()
  })
})

describe('sameDrop', () => {
  it('compares drops by what they would do', () => {
    const into: Drop = { kind: 'into', group: 'a', slot: 1 }
    const sameInto: Drop = { kind: 'into', group: 'a', slot: 1 }
    const otherSlot: Drop = { kind: 'into', group: 'a', slot: 2 }

    expect(sameDrop(between(2, null), between(2, 't'))).toBe(false)
    expect(sameDrop(between(2, 't'), between(2, 't'))).toBe(true)
    expect(sameDrop(into, sameInto)).toBe(true)
    expect(sameDrop(into, otherSlot)).toBe(false)
    expect(sameDrop(between(1, null), into)).toBe(false)
    expect(sameDrop(null, null)).toBe(true)
    expect(sameDrop(null, into)).toBe(false)
  })
})
