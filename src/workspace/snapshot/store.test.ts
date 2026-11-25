import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Workspace } from '@protocol/arrangement'
import type { AgentSession } from './format'
import { makeTempDirectory, removeTempDirectories } from '@testing/tempDirectory'
import { AgentSessions } from '../agentSessions'
import { readSnapshot } from './files'
import { SnapshotStore } from './store'

const PANE_ID = 'pty-1'
const SESSION_A: AgentSession = {
  harness: 'claude',
  sessionId: '11111111-1111-4111-8111-111111111111',
  cwd: '/tmp/a'
}
const SESSION_B: AgentSession = {
  harness: 'claude',
  sessionId: '22222222-2222-4222-8222-222222222222',
  cwd: '/tmp/b'
}
const SESSION_C: AgentSession = {
  harness: 'claude',
  sessionId: '33333333-3333-4333-8333-333333333333',
  cwd: '/tmp/c'
}
const WORKSPACE: Workspace = {
  groups: [
    {
      id: 'group-1',
      panes: [{ id: PANE_ID, title: '', width: 1, color: null }],
      activePane: 0,
      tabGroup: null
    }
  ],
  activeGroup: 0,
  tabGroups: []
}

describe('SnapshotStore Claude sessions', () => {
  let directory: string
  let now: number
  let agentSessions: AgentSessions
  let store: SnapshotStore

  function agentInFile(): AgentSession | null {
    vi.advanceTimersByTime(300)
    const snapshot = readSnapshot(directory)
    if (snapshot === null) {
      throw new Error('no snapshot was written')
    }
    return snapshot.groups[0].panes[0].agent
  }

  beforeEach(() => {
    vi.useFakeTimers()
    directory = makeTempDirectory('taco-shells-store-test-')
    now = 0
    const clock = (): number => now
    agentSessions = new AgentSessions(clock)
    const directoryOf = (): Promise<string | null> => Promise.resolve(null)
    store = new SnapshotStore(directory, agentSessions, directoryOf)
    store.workspaceChanged(WORKSPACE)
  })

  afterEach(() => {
    store.stop()
    vi.useRealTimers()
    removeTempDirectories()
  })

  it('writes a seeded directory before the first refresh has run', () => {
    store.seedDirectory(PANE_ID, '/tmp/seeded')
    vi.advanceTimersByTime(300)
    const snapshot = readSnapshot(directory)
    expect(snapshot?.groups[0].panes[0].cwd).toBe('/tmp/seeded')
  })

  it('resumes the newest session when an older one died without an end', () => {
    agentSessions.recordStart(PANE_ID, SESSION_A)
    agentSessions.recordStart(PANE_ID, SESSION_B)

    expect(agentInFile()).toEqual(SESSION_B)
  })

  it('keeps the older session when the newer one ends', () => {
    agentSessions.recordStart(PANE_ID, SESSION_A)
    agentSessions.recordStart(PANE_ID, SESSION_B)
    agentSessions.recordEnd(PANE_ID, 'claude',SESSION_B.sessionId)

    expect(agentInFile()).toEqual(SESSION_A)
  })

  it('follows /clear whichever of the end and the start arrives first', () => {
    agentSessions.recordStart(PANE_ID, SESSION_A)
    agentSessions.recordEnd(PANE_ID, 'claude',SESSION_A.sessionId)
    agentSessions.recordStart(PANE_ID, SESSION_C)
    expect(agentInFile()).toEqual(SESSION_C)

    agentSessions.recordEnd(PANE_ID, 'claude',SESSION_C.sessionId)
    agentSessions.recordStart(PANE_ID, SESSION_A)
    agentSessions.recordStart(PANE_ID, SESSION_C)
    agentSessions.recordEnd(PANE_ID, 'claude',SESSION_A.sessionId)
    expect(agentInFile()).toEqual(SESSION_C)
  })

  it('keeps a seeded session once a start confirms it', () => {
    agentSessions.seed(PANE_ID, SESSION_A)
    agentSessions.recordStart(PANE_ID, SESSION_A)
    now = 120_000

    expect(agentInFile()).toEqual(SESSION_A)
  })

  it('drops a seeded session that is never confirmed', () => {
    agentSessions.seed(PANE_ID, SESSION_A)
    now = 60_001

    expect(agentInFile()).toBeNull()
  })

  it('ignores an end and a pty exit after freeze, and flushAtQuit still writes the entry', async () => {
    agentSessions.recordStart(PANE_ID, SESSION_A)
    store.freeze()
    agentSessions.freeze()
    agentSessions.recordEnd(PANE_ID, 'claude',SESSION_A.sessionId)
    agentSessions.forget(PANE_ID)
    store.forget(PANE_ID)
    await store.flushAtQuit()

    const snapshot = readSnapshot(directory)
    expect(snapshot?.groups[0].panes[0].agent).toEqual(SESSION_A)
  })

  it('clears a pane entry when its pty exits', () => {
    agentSessions.recordStart(PANE_ID, SESSION_A)
    agentSessions.forget(PANE_ID)
    store.forget(PANE_ID)
    store.workspaceChanged(WORKSPACE)

    expect(agentInFile()).toBeNull()
  })
})
