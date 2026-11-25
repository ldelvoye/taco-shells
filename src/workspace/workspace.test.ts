import { describe, expect, it } from 'vitest'

import type { Workspace as Arrangement } from '@protocol/arrangement'
import type { Snapshot, SnapshotGroup } from '@protocol/snapshot'
import type { DrawnState } from '@protocol/workspace'
import { Workspace } from './workspace'

const SESSION = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'

interface Recording {
  workspace: Workspace
  states: DrawnState[]
  arrangements: Arrangement[]
  emptied: { count: number }
}

function recordingWorkspace(snapshot: Snapshot | null = null): Recording {
  const states: DrawnState[] = []
  const arrangements: Arrangement[] = []
  const emptied = { count: 0 }
  const workspace = new Workspace(snapshot, {
    onState: (state) => {
      states.push(state)
    },
    onArrangement: (arrangement) => {
      arrangements.push(arrangement)
    },
    onEmptied: () => {
      emptied.count += 1
    }
  })
  return { workspace, states, arrangements, emptied }
}

function opened(...panes: string[]): Recording {
  const recording = recordingWorkspace()
  for (const pane of panes) {
    recording.workspace.request({ type: 'terminalOpened', pane })
  }
  return recording
}

function singlePaneGroup(): SnapshotGroup {
  return {
    panes: [{ width: 1, cwd: null, agent: null, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function twoEntrySnapshot(): Snapshot {
  return {
    version: 1,
    groups: [singlePaneGroup(), singlePaneGroup()],
    activeGroup: 0,
    tabGroups: []
  }
}

function groupIdsOf(workspace: Workspace): string[] {
  const arrangement = workspace.drawn().workspace
  if (arrangement === null) {
    return []
  }
  return arrangement.groups.map((group) => group.id)
}

function paneIdsOf(workspace: Workspace): string[][] {
  const arrangement = workspace.drawn().workspace
  if (arrangement === null) {
    return []
  }
  return arrangement.groups.map((group) => group.panes.map((pane) => pane.id))
}

function activePaneIdOf(workspace: Workspace): string | undefined {
  const arrangement = workspace.drawn().workspace
  if (arrangement === null) {
    return undefined
  }
  const group = arrangement.groups[arrangement.activeGroup]
  return group.panes[group.activePane].id
}

describe('Workspace', () => {
  it('places a reported terminal under a group id it mints', () => {
    const { workspace, states } = recordingWorkspace()

    const first = workspace.request({ type: 'terminalOpened', pane: 'p1' })
    const second = workspace.request({ type: 'terminalOpened', pane: 'p2' })

    expect(first).toEqual({ ok: true, tabGroup: null })
    expect(second).toEqual({ ok: true, tabGroup: null })
    expect(groupIdsOf(workspace)).toEqual(['group-1', 'group-2'])
    expect(states.map((state) => state.revision)).toEqual([1, 2])
  })

  it('refuses a split it cannot place', () => {
    const { workspace, states } = opened('p1')
    const statesBefore = states.length

    const refused = workspace.request({ type: 'splitSpawned', donor: 'gone', pane: 'p2' })
    expect(refused.ok).toBe(false)
    expect(states.length).toBe(statesBefore)

    const placed = workspace.request({ type: 'splitSpawned', donor: 'p1', pane: 'p2' })
    expect(placed.ok).toBe(true)
    expect(paneIdsOf(workspace)).toEqual([['p1', 'p2']])
  })

  it("applies the window's focus move and says which one it has applied", () => {
    const { workspace, states } = opened('p1', 'p2')
    expect(activePaneIdOf(workspace)).toBe('p2')

    workspace.request({ type: 'focusReported', pane: 'p1', move: 3 })
    expect(activePaneIdOf(workspace)).toBe('p1')
    expect(states[states.length - 1].focusMove).toBe(3)

    const statesBefore = states.length
    const arrangementBefore = workspace.drawn().workspace
    workspace.request({ type: 'focusReported', pane: 'p1', move: 4 })
    expect(states.length).toBe(statesBefore + 1)
    expect(states[states.length - 1].focusMove).toBe(4)
    expect(workspace.drawn().workspace).toBe(arrangementBefore)

    workspace.request({ type: 'focusReported', pane: 'p1', move: 2 })
    expect(workspace.drawn().focusMove).toBe(4)
  })

  it('creates a tab group and answers with its id', () => {
    const { workspace } = opened('p1')

    const created = workspace.request({ type: 'tabGroupCreated', pane: 'p1' })
    expect(created.tabGroup).toBe('tab-group-1')

    const unknown = workspace.request({ type: 'tabGroupCreated', pane: 'gone' })
    expect(unknown.tabGroup).toBeNull()
  })

  it("mints the new entry's id for a drop", () => {
    const { workspace } = opened('p1')
    workspace.request({ type: 'splitSpawned', donor: 'p1', pane: 'p2' })

    workspace.request({
      type: 'dropped',
      dragged: { kind: 'pane', pane: 'p2' },
      drop: { kind: 'between', index: 1, tabGroup: null }
    })

    expect(groupIdsOf(workspace)).toEqual(['group-1', 'group-2'])
    expect(paneIdsOf(workspace)).toEqual([['p1'], ['p2']])
  })

  it('rebuilds the saved arrangement around the terminals the window reports', () => {
    const rebuilt = recordingWorkspace(twoEntrySnapshot())
    const placed = rebuilt.workspace.request({ type: 'restored', panes: ['a', 'b'] })
    expect(placed.ok).toBe(true)
    expect(paneIdsOf(rebuilt.workspace)).toEqual([['a'], ['b']])

    const short = recordingWorkspace(twoEntrySnapshot())
    const refused = short.workspace.request({ type: 'restored', panes: ['a'] })
    expect(refused.ok).toBe(false)
    expect(short.workspace.drawn().workspace).toBeNull()
  })

  it('publishes rows as a plain object', () => {
    const { workspace } = opened('p1')

    workspace.agentSignalled({ id: 'p1', harness: 'claude', sessionId: SESSION, signal: 'start' })

    const drawn = workspace.drawn()
    expect(Object.keys(drawn.agentRows)).toEqual(['p1'])
    expect(JSON.parse(JSON.stringify(drawn))).toEqual(drawn)
  })

  it('says when the last terminal has gone, once', () => {
    const { workspace, emptied } = opened('p1')

    workspace.terminalExited('p1')
    expect(emptied.count).toBe(1)

    workspace.terminalExited('p1')
    expect(emptied.count).toBe(1)
  })

  it('hands the keyboard back when a hook asks for a terminal that is already active', () => {
    const { workspace, states } = opened('p1', 'p2')
    const before = workspace.drawn()

    workspace.focusRequested('p2')
    const after = workspace.drawn()
    expect(after.refocus).toBe(before.refocus + 1)
    expect(after.workspace).toBe(before.workspace)

    const statesBefore = states.length
    workspace.appActivated()
    expect(states.length).toBe(statesBefore)
  })

  it('reports the arrangement to save only when it changed', () => {
    const { workspace, arrangements } = opened('p1')
    const savesBefore = arrangements.length

    workspace.request({ type: 'titleChanged', pane: 'p1', title: 'renamed' })
    expect(arrangements.length).toBe(savesBefore + 1)

    workspace.agentSignalled({ id: 'p1', harness: 'claude', sessionId: SESSION, signal: 'start' })
    expect(arrangements.length).toBe(savesBefore + 1)
  })
})
