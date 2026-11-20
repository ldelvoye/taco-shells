import { describe, expect, it } from 'vitest'

import type { Group, Workspace } from './model'
import { emptyWorkspace } from './model'
import {
  isSessionUuid,
  paneCount,
  parseSnapshot,
  snapshotFrom,
  workspaceFromSnapshot
} from './snapshot'
import type { ClaudeSession, Snapshot, SnapshotTabGroup } from './snapshot'

const SESSION_ID = '123e4567-e89b-12d3-a456-426614174000'

function widthSum(snapshot: Snapshot, group: number): number {
  const widths = snapshot.groups[group].panes.map((pane) => pane.width)
  return widths.reduce((sum, width) => sum + width, 0)
}

describe('isSessionUuid', () => {
  it('accepts a UUID in either case and rejects anything else', () => {
    expect(isSessionUuid(SESSION_ID)).toBe(true)
    expect(isSessionUuid(SESSION_ID.toUpperCase())).toBe(true)
    expect(isSessionUuid(`${SESSION_ID}\n`)).toBe(false)
    expect(isSessionUuid('--dangerously-skip-permissions')).toBe(false)
    expect(isSessionUuid(42)).toBe(false)
  })
})

describe('parseSnapshot', () => {
  it('yields null for text that cannot be a snapshot', () => {
    expect(parseSnapshot('')).toBeNull()
    expect(parseSnapshot('{not json')).toBeNull()
    expect(parseSnapshot('[]')).toBeNull()
    expect(parseSnapshot('null')).toBeNull()
    expect(parseSnapshot(JSON.stringify({ version: 2, groups: [], activeGroup: 0 }))).toBeNull()
    expect(parseSnapshot(JSON.stringify({ version: 1, groups: {}, activeGroup: 0 }))).toBeNull()
  })

  it('keeps a valid file with no groups as an empty snapshot', () => {
    const text = JSON.stringify({ version: 1, groups: [], activeGroup: 3 })
    expect(parseSnapshot(text)).toEqual({ version: 1, groups: [], activeGroup: 0, tabGroups: [] })
  })

  it('normalises a hand-edited file into a sane one', () => {
    const text = JSON.stringify({
      version: 1,
      activeGroup: 99,
      groups: [
        { panes: [], activePane: 0 },
        {
          activePane: -4,
          panes: [
            { width: 1, cwd: '/a', claude: { sessionId: SESSION_ID, cwd: '/a' }, color: 'blue' },
            { width: 1, cwd: 7, claude: { sessionId: 'nope', cwd: '/b' }, color: 'mauve' },
            'garbage',
            { width: -2, cwd: '/c', claude: { sessionId: SESSION_ID, cwd: '' } }
          ]
        },
        { activePane: 1.5, panes: [{ width: 0.2, cwd: null, claude: null }] }
      ]
    })

    const snapshot = parseSnapshot(text)
    if (snapshot === null) {
      throw new Error('expected a snapshot')
    }

    expect(snapshot.groups).toHaveLength(2)
    expect(snapshot.activeGroup).toBe(1)

    const [first, second] = snapshot.groups
    expect(first.activePane).toBe(0)
    expect(first.panes).toHaveLength(3)
    expect(first.panes[0].claude).toEqual({ sessionId: SESSION_ID, cwd: '/a' })
    expect(first.panes[1].cwd).toBeNull()
    expect(first.panes[1].claude).toBeNull()
    expect(first.panes[2].claude).toBeNull()
    expect(first.panes.map((pane) => pane.color)).toEqual(['blue', null, null])
    expect(widthSum(snapshot, 0)).toBeCloseTo(1)
    expect(first.panes[2].width).toBeGreaterThan(0)

    expect(second.activePane).toBe(0)
    expect(second.panes[0].width).toBe(1)
  })

  it('clamps an out-of-range activePane to the last pane', () => {
    const pane = { width: 0.5, cwd: null, claude: null }
    const text = JSON.stringify({
      version: 1,
      activeGroup: 0,
      groups: [{ activePane: 9, panes: [pane, pane] }]
    })
    expect(parseSnapshot(text)?.groups[0].activePane).toBe(1)
  })

  it('keeps at most as many panes as the width floor allows', () => {
    const pane = { width: 1, cwd: null, claude: null }
    const panes = Array.from({ length: 14 }, () => pane)
    const text = JSON.stringify({ version: 1, activeGroup: 0, groups: [{ activePane: 13, panes }] })
    const parsed = parseSnapshot(text)
    expect(parsed?.groups[0].panes).toHaveLength(10)
    expect(parsed?.groups[0].activePane).toBe(9)
  })

  it('loads a file saved before tab groups existed', () => {
    const text =
      '{"version":1,"groups":[{"panes":[{"width":1,"cwd":null,"claude":null,"color":null}],"activePane":0}],"activeGroup":0}'

    const snapshot = parseSnapshot(text)

    expect(snapshot?.groups).toHaveLength(1)
    expect(snapshot?.groups[0].tabGroup).toBeNull()
    expect(snapshot?.tabGroups).toEqual([])
  })

  it('repairs a hand-edited file', () => {
    const pane = { width: 1, cwd: null, claude: null }
    const record: SnapshotTabGroup = { name: 'a', color: 'blue', collapsed: false }
    const cases: [string, unknown[], unknown[], Array<number | null>, SnapshotTabGroup[]][] = [
      ['split across the list', [0, null, 0], [record], [0, null, null], [record]],
      ['index out of range', [5], [record], [null], []],
      ['unknown color', [0], [{ ...record, color: 'mauve' }], [0], [{ ...record, color: 'red' }]],
      ['non-string name', [0], [{ ...record, name: 7 }], [0], [{ ...record, name: '' }]],
      ['record nothing points at', [1], [record, { ...record, name: 'b' }], [0], [{ ...record, name: 'b' }]]
    ]

    for (const [label, indexes, records, expectedIndexes, expectedRecords] of cases) {
      const groups = indexes.map((tabGroup) => {
        return { panes: [pane], activePane: 0, tabGroup }
      })
      const text = JSON.stringify({ version: 1, activeGroup: 0, groups, tabGroups: records })

      const snapshot = parseSnapshot(text)

      expect(snapshot?.groups.map((group) => group.tabGroup), label).toEqual(expectedIndexes)
      expect(snapshot?.tabGroups, label).toEqual(expectedRecords)
    }
  })
})

describe('snapshot and workspace round trip', () => {
  const claude: ClaudeSession = { sessionId: SESSION_ID, cwd: '/work' }
  const workspace: Workspace = {
    activeGroup: 1,
    tabGroups: [],
    groups: [
      {
        id: 'g1',
        activePane: 0,
        tabGroup: null,
        panes: [{ id: 'pty-1', title: 'one', width: 1, color: null }]
      },
      {
        id: 'g2',
        activePane: 1,
        tabGroup: null,
        panes: [
          { id: 'pty-2', title: 'two', width: 0.25, color: 'purple' },
          { id: 'pty-3', title: 'three', width: 0.75, color: null }
        ]
      }
    ]
  }

  function cwdOf(id: string): string | null {
    if (id === 'pty-2') {
      return null
    }
    return `/dir/${id}`
  }

  function claudeOf(id: string): ClaudeSession | null {
    if (id === 'pty-3') {
      return claude
    }
    return null
  }

  it('rebuilds the same layout under fresh ids, with empty titles', () => {
    const snapshot = snapshotFrom(workspace, cwdOf, claudeOf)
    const reparsed = parseSnapshot(JSON.stringify(snapshot))
    expect(reparsed).toEqual(snapshot)

    const restored = workspaceFromSnapshot(snapshot, ['a', 'b', 'c'], ['x', 'y'], [])
    expect(restored.activeGroup).toBe(1)
    expect(restored.groups.map((group) => group.id)).toEqual(['x', 'y'])
    expect(restored.groups[1].activePane).toBe(1)
    expect(restored.groups[1].panes).toEqual([
      { id: 'b', title: '', width: 0.25, color: 'purple' },
      { id: 'c', title: '', width: 0.75, color: null }
    ])
    expect(snapshot.groups[1].panes[1].claude).toEqual(claude)
    expect(snapshot.groups[1].panes[0].cwd).toBeNull()
  })

  it('falls back to an empty workspace when the id counts do not match', () => {
    const snapshot = snapshotFrom(workspace, cwdOf, claudeOf)
    expect(paneCount(snapshot)).toBe(3)
    expect(workspaceFromSnapshot(snapshot, ['a', 'b'], ['x', 'y'], [])).toEqual(emptyWorkspace())
    expect(workspaceFromSnapshot(snapshot, ['a', 'b', 'c'], ['x'], [])).toEqual(emptyWorkspace())
  })
})

describe('tab groups through a save and a restore', () => {
  function entry(id: string, tabGroup: string | null): Group {
    return {
      id,
      panes: [{ id: `pane-${id}`, title: '', width: 1, color: null }],
      activePane: 0,
      tabGroup
    }
  }

  it('keeps tab groups through a save and a restore', () => {
    const workspace: Workspace = {
      groups: [entry('a', 't1'), entry('b', 't1'), entry('c', null), entry('d', 't2')],
      activeGroup: 2,
      tabGroups: [
        { id: 't1', name: 'pebbles', color: 'blue', collapsed: false },
        { id: 't2', name: 'smorg', color: 'orange', collapsed: true }
      ]
    }
    const noCwd = (): null => null
    const noClaude = (): null => null

    const snapshot = snapshotFrom(workspace, noCwd, noClaude)
    expect(snapshot.tabGroups).toHaveLength(2)

    const reparsed = parseSnapshot(JSON.stringify(snapshot))
    if (reparsed === null) {
      throw new Error('expected a snapshot')
    }

    const tooFewIds = workspaceFromSnapshot(reparsed, ['p1', 'p2', 'p3', 'p4'], ['g1', 'g2', 'g3', 'g4'], ['u1'])
    expect(tooFewIds.groups).toEqual([])

    const restored = workspaceFromSnapshot(reparsed, ['p1', 'p2', 'p3', 'p4'], ['g1', 'g2', 'g3', 'g4'], ['u1', 'u2'])

    expect(restored.groups.map((group) => group.tabGroup)).toEqual(['u1', 'u1', null, 'u2'])
    expect(restored.activeGroup).toBe(2)
    expect(restored.tabGroups).toEqual([
      { id: 'u1', name: 'pebbles', color: 'blue', collapsed: false },
      { id: 'u2', name: 'smorg', color: 'orange', collapsed: true }
    ])
  })

  it('opens the group of a restored active terminal', () => {
    const pane = { width: 1, cwd: null, claude: null }
    const text = JSON.stringify({
      version: 1,
      activeGroup: 0,
      groups: [
        { panes: [pane], activePane: 0, tabGroup: 0 },
        { panes: [pane], activePane: 0, tabGroup: null }
      ],
      tabGroups: [{ name: 'a', color: 'blue', collapsed: true }]
    })
    const snapshot = parseSnapshot(text)
    if (snapshot === null) {
      throw new Error('expected a snapshot')
    }

    const restored = workspaceFromSnapshot(snapshot, ['p1', 'p2'], ['g1', 'g2'], ['u1'])

    expect(restored.tabGroups[0].collapsed).toBe(false)
  })
})
