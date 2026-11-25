import { describe, expect, it } from 'vitest'

import type { Group } from '@protocol/arrangement'
import type { AgentSignalEvent } from '@protocol/ipc'
import type { AgentSignal } from '@protocol/rows'
import { emptyWorkspace } from '@rules/arrangement'
import { addGroup } from '@rules/transitions/arrangement'
import {
  initialWorkspaceState,
  reduceWorkspaceState,
  type WorkspaceEvent,
  type WorkspaceState
} from './workspaceState'

const SESSION = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'

function groupOf(id: string, pane: string): Group {
  return { id, panes: [{ id: pane, title: '', width: 1, color: null }], activePane: 0, tabGroup: null }
}

function dispatch(state: WorkspaceState, ...events: WorkspaceEvent[]): WorkspaceState {
  return events.reduce(reduceWorkspaceState, state)
}

function signal(pane: string, kind: AgentSignal): WorkspaceEvent {
  const event: AgentSignalEvent = { id: pane, harness: 'claude', sessionId: SESSION, signal: kind }
  return { type: 'agentSignalled', signal: event }
}

function twoTerminals(): WorkspaceState {
  return dispatch(
    initialWorkspaceState(),
    { type: 'terminalOpened', group: groupOf('g1', 'p1') },
    { type: 'terminalOpened', group: groupOf('g2', 'p2') }
  )
}

function titleOf(state: WorkspaceState, pane: string): string | undefined {
  const panes = state.workspace?.groups.flatMap((group) => group.panes)
  return panes?.find((candidate) => candidate.id === pane)?.title
}

function activePaneId(state: WorkspaceState): string | undefined {
  const workspace = state.workspace
  if (workspace === null) {
    return undefined
  }
  const group = workspace.groups[workspace.activeGroup]
  return group.panes[group.activePane].id
}

describe('workspace state', () => {
  it('updates every slice together when a pane exits', () => {
    const before = dispatch(
      twoTerminals(),
      signal('p1', 'start'),
      { type: 'attentionRequested', pane: 'p1' },
      { type: 'titleChanged', pane: 'p1', title: 'build' }
    )
    expect(before.waiting).toBe('p1')

    const after = reduceWorkspaceState(before, { type: 'paneExited', pane: 'p1' })

    const paneIds = after.workspace?.groups.flatMap((group) => group.panes.map((p) => p.id))
    expect(paneIds).toEqual(['p2'])
    expect(after.agentRows.has('p1')).toBe(false)
    expect(after.waiting).toBeNull()
    expect(after.rawTitles.has('p1')).toBe(false)
  })

  it('reads a title by its row agent and replays the last raw title on session start', () => {
    const titled = dispatch(twoTerminals(), { type: 'titleChanged', pane: 'p1', title: '✳ Fix bug' })
    expect(titleOf(titled, 'p1')).toBe('✳ Fix bug')

    const started = reduceWorkspaceState(titled, signal('p1', 'start'))
    expect(titleOf(started, 'p1')).toBe('Fix bug')

    const working = reduceWorkspaceState(started, signal('p1', 'working'))
    expect(working.agentRows.get('p1')?.state).toBe('working')

    const idle = reduceWorkspaceState(working, { type: 'titleChanged', pane: 'p1', title: '✳ Fix bug' })
    expect(idle.agentRows.get('p1')?.state).toBe('idle')

    const titledP2 = reduceWorkspaceState(idle, { type: 'titleChanged', pane: 'p2', title: '✳ Ship it' })
    const resumed = reduceWorkspaceState(titledP2, signal('p2', 'resume'))
    expect(titleOf(resumed, 'p2')).toBe('Ship it')
  })

  it('bumps refocus on a drop or a toggle only when the workspace changed', () => {
    const grouped = reduceWorkspaceState(twoTerminals(), {
      type: 'tabGroupCreated',
      entry: 'g1',
      id: 't1'
    })
    expect(grouped.refocus).toBe(0)

    const moved = reduceWorkspaceState(grouped, {
      type: 'dropped',
      dragged: { kind: 'group', group: 'g2' },
      drop: { kind: 'between', index: 0, tabGroup: null },
      newGroup: 'g3'
    })
    expect(moved.refocus).toBe(1)

    const missed = reduceWorkspaceState(moved, {
      type: 'dropped',
      dragged: { kind: 'pane', pane: 'nobody' },
      drop: { kind: 'between', index: 0, tabGroup: null },
      newGroup: 'g4'
    })
    expect(missed.refocus).toBe(1)

    const toggled = reduceWorkspaceState(moved, { type: 'tabGroupToggled', id: 't1' })
    expect(toggled.refocus).toBe(2)

    const toggledMissing = reduceWorkspaceState(toggled, { type: 'tabGroupToggled', id: 'nope' })
    expect(toggledMissing.refocus).toBe(2)
  })

  it('drops a workspace edit but keeps an agent signal and a title before the first terminal', () => {
    const initial = initialWorkspaceState()
    expect(reduceWorkspaceState(initial, { type: 'focusMoved', move: 'nextPane' })).toBe(initial)

    const kept = dispatch(initial, signal('p1', 'start'), {
      type: 'titleChanged',
      pane: 'p1',
      title: 'build'
    })
    expect(kept.agentRows.has('p1')).toBe(true)
    expect(kept.rawTitles.get('p1')).toBe('build')

    const workspace = addGroup(emptyWorkspace(), groupOf('g1', 'p1'))
    const restored = reduceWorkspaceState(kept, { type: 'workspaceRestored', workspace })
    expect(restored.agentRows.has('p1')).toBe(true)
    expect(restored.rawTitles.get('p1')).toBe('build')
  })

  it('jumps to the waiting pane and clears it when the app is activated', () => {
    const asked = reduceWorkspaceState(twoTerminals(), { type: 'attentionRequested', pane: 'p1' })
    expect(asked.waiting).toBe('p1')

    const activated = reduceWorkspaceState(asked, { type: 'appActivated' })
    expect(activePaneId(activated)).toBe('p1')
    expect(activated.waiting).toBeNull()

    const askedActive = reduceWorkspaceState(activated, { type: 'attentionRequested', pane: 'p1' })
    expect(askedActive.waiting).toBeNull()
  })

  it('turns a done row idle once its pane is on screen', () => {
    const done = dispatch(twoTerminals(), signal('p1', 'start'), signal('p1', 'working'), signal('p1', 'stopped'))
    expect(done.agentRows.get('p1')?.state).toBe('done')

    const shown = reduceWorkspaceState(done, { type: 'focusMoved', move: 'previousGroup' })
    expect(shown.agentRows.get('p1')?.state).toBe('idle')

    const stoppedOnScreen = dispatch(done, signal('p2', 'start'), signal('p2', 'working'), signal('p2', 'stopped'))
    expect(stoppedOnScreen.agentRows.get('p2')?.state).toBe('idle')
  })

  it('returns the same state object when an event changes nothing', () => {
    const titled = dispatch(
      twoTerminals(),
      signal('p1', 'start'),
      { type: 'titleChanged', pane: 'p2', title: 'build' }
    )

    expect(reduceWorkspaceState(titled, { type: 'paneActivated', pane: 'p2' })).toBe(titled)
    expect(reduceWorkspaceState(titled, { type: 'titleChanged', pane: 'p2', title: 'build' })).toBe(titled)
    expect(reduceWorkspaceState(titled, signal('p3', 'working'))).toBe(titled)
  })
})
