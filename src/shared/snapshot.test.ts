import { describe, expect, it } from 'vitest'

import type { Workspace } from './model'
import { emptyWorkspace } from './model'
import {
  isSessionUuid,
  paneCount,
  parseSnapshot,
  snapshotFrom,
  workspaceFromSnapshot
} from './snapshot'
import type { ClaudeSession, Snapshot } from './snapshot'

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
    expect(parseSnapshot(text)).toEqual({ version: 1, groups: [], activeGroup: 0 })
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
            { width: 1, cwd: '/a', claude: { sessionId: SESSION_ID, cwd: '/a' } },
            { width: 1, cwd: 7, claude: { sessionId: 'nope', cwd: '/b' } },
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
})

describe('snapshot and workspace round trip', () => {
  const claude: ClaudeSession = { sessionId: SESSION_ID, cwd: '/work' }
  const workspace: Workspace = {
    activeGroup: 1,
    groups: [
      { id: 'g1', activePane: 0, panes: [{ id: 'pty-1', title: 'one', width: 1 }] },
      {
        id: 'g2',
        activePane: 1,
        panes: [
          { id: 'pty-2', title: 'two', width: 0.25 },
          { id: 'pty-3', title: 'three', width: 0.75 }
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

    const restored = workspaceFromSnapshot(snapshot, ['a', 'b', 'c'], ['x', 'y'])
    expect(restored.activeGroup).toBe(1)
    expect(restored.groups.map((group) => group.id)).toEqual(['x', 'y'])
    expect(restored.groups[1].activePane).toBe(1)
    expect(restored.groups[1].panes).toEqual([
      { id: 'b', title: '', width: 0.25 },
      { id: 'c', title: '', width: 0.75 }
    ])
    expect(snapshot.groups[1].panes[1].claude).toEqual(claude)
    expect(snapshot.groups[1].panes[0].cwd).toBeNull()
  })

  it('falls back to an empty workspace when the id counts do not match', () => {
    const snapshot = snapshotFrom(workspace, cwdOf, claudeOf)
    expect(paneCount(snapshot)).toBe(3)
    expect(workspaceFromSnapshot(snapshot, ['a', 'b'], ['x', 'y'])).toEqual(emptyWorkspace())
    expect(workspaceFromSnapshot(snapshot, ['a', 'b', 'c'], ['x'])).toEqual(emptyWorkspace())
  })
})
