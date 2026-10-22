import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Workspace } from '@shared/model'
import type { ClaudeSession } from '@shared/snapshot'
import { readSnapshot } from './files'
import { SnapshotStore } from './store'

const PANE_ID = 'pty-1'
const SESSION_A: ClaudeSession = { sessionId: '11111111-1111-4111-8111-111111111111', cwd: '/tmp/a' }
const SESSION_B: ClaudeSession = { sessionId: '22222222-2222-4222-8222-222222222222', cwd: '/tmp/b' }
const SESSION_C: ClaudeSession = { sessionId: '33333333-3333-4333-8333-333333333333', cwd: '/tmp/c' }
const WORKSPACE: Workspace = {
  groups: [{ id: 'group-1', panes: [{ id: PANE_ID, title: '', width: 1 }], activePane: 0 }],
  activeGroup: 0
}

describe('SnapshotStore Claude sessions', () => {
  let directory: string
  let now: number
  let exitListener: (event: { id: string; exitCode: number }) => void
  let store: SnapshotStore

  function claudeInFile(): ClaudeSession | null {
    vi.advanceTimersByTime(300)
    const snapshot = readSnapshot(directory)
    if (snapshot === null) {
      throw new Error('no snapshot was written')
    }
    return snapshot.groups[0].panes[0].claude
  }

  beforeEach(() => {
    vi.useFakeTimers()
    directory = mkdtempSync(join(tmpdir(), 'taco-shells-store-test-'))
    now = 0
    const clock = (): number => now
    const sessions = {
      pidOf: () => null,
      onExit: (listener: typeof exitListener) => {
        exitListener = listener
      }
    }
    store = new SnapshotStore(directory, sessions, clock)
    store.workspaceChanged(WORKSPACE)
  })

  afterEach(() => {
    store.stop()
    vi.useRealTimers()
  })

  it('writes a seeded directory before the first refresh has run', () => {
    store.seedDirectory(PANE_ID, '/tmp/seeded')
    vi.advanceTimersByTime(300)
    const snapshot = readSnapshot(directory)
    expect(snapshot?.groups[0].panes[0].cwd).toBe('/tmp/seeded')
  })

  it('resumes the newest session when an older one died without an end', () => {
    store.recordClaudeStart(PANE_ID, SESSION_A)
    store.recordClaudeStart(PANE_ID, SESSION_B)

    expect(claudeInFile()).toEqual(SESSION_B)
  })

  it('keeps the older session when the newer one ends', () => {
    store.recordClaudeStart(PANE_ID, SESSION_A)
    store.recordClaudeStart(PANE_ID, SESSION_B)
    store.recordClaudeEnd(PANE_ID, SESSION_B.sessionId)

    expect(claudeInFile()).toEqual(SESSION_A)
  })

  it('follows /clear whichever of the end and the start arrives first', () => {
    store.recordClaudeStart(PANE_ID, SESSION_A)
    store.recordClaudeEnd(PANE_ID, SESSION_A.sessionId)
    store.recordClaudeStart(PANE_ID, SESSION_C)
    expect(claudeInFile()).toEqual(SESSION_C)

    store.recordClaudeEnd(PANE_ID, SESSION_C.sessionId)
    store.recordClaudeStart(PANE_ID, SESSION_A)
    store.recordClaudeStart(PANE_ID, SESSION_C)
    store.recordClaudeEnd(PANE_ID, SESSION_A.sessionId)
    expect(claudeInFile()).toEqual(SESSION_C)
  })

  it('keeps a seeded session once a start confirms it', () => {
    store.seedClaude(PANE_ID, SESSION_A)
    store.recordClaudeStart(PANE_ID, SESSION_A)
    now = 120_000

    expect(claudeInFile()).toEqual(SESSION_A)
  })

  it('drops a seeded session that is never confirmed', () => {
    store.seedClaude(PANE_ID, SESSION_A)
    now = 60_001

    expect(claudeInFile()).toBeNull()
  })

  it('ignores an end after freeze, and flushAtQuit still writes the entry', async () => {
    store.recordClaudeStart(PANE_ID, SESSION_A)
    store.freeze()
    store.recordClaudeEnd(PANE_ID, SESSION_A.sessionId)
    await store.flushAtQuit()

    const snapshot = readSnapshot(directory)
    expect(snapshot?.groups[0].panes[0].claude).toEqual(SESSION_A)
  })

  it('clears a pane entry when its pty exits', () => {
    store.recordClaudeStart(PANE_ID, SESSION_A)
    exitListener({ id: PANE_ID, exitCode: 0 })
    store.workspaceChanged(WORKSPACE)

    expect(claudeInFile()).toBeNull()
  })
})
