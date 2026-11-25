import { homedir, tmpdir } from 'node:os'
import { describe, expect, it } from 'vitest'

import type { PtyExitEvent } from '@protocol/ipc'
import type { TerminalStart, TerminalStarted } from '@protocol/runtime'
import { PtySessions, type SpawnedPty, type SpawnPty } from './sessions'

interface Spawned {
  cols: number | undefined
  rows: number | undefined
  cwd: string | undefined
}

interface FakeSpawn {
  spawned: Spawned[]
  written: string[]
  spawn: SpawnPty
}

function fakeSpawn(failure: Error | null = null): FakeSpawn {
  const spawned: Spawned[] = []
  const written: string[] = []
  const spawn: SpawnPty = (_file, _args, options) => {
    if (failure !== null) {
      throw failure
    }
    spawned.push({ cols: options.cols, rows: options.rows, cwd: options.cwd })
    const pty: SpawnedPty = {
      pid: 1,
      write: (data) => {
        written.push(String(data))
      },
      resize: () => undefined,
      kill: () => undefined,
      onData: () => ({ dispose: () => undefined }),
      onExit: () => ({ dispose: () => undefined })
    }
    return pty
  }
  return { spawned, written, spawn }
}

function startOf(id: string, overrides: Partial<TerminalStart> = {}): TerminalStart {
  return {
    id,
    directory: null,
    directoryOf: null,
    command: null,
    size: { cols: 80, rows: 24 },
    ...overrides
  }
}

function nextExits(sessions: PtySessions, count: number): Promise<PtyExitEvent[]> {
  const exits: PtyExitEvent[] = []
  return new Promise((resolve) => {
    sessions.onExit((event) => {
      exits.push(event)
      if (exits.length === count) {
        resolve(exits)
      }
    })
  })
}

function nextStarts(sessions: PtySessions, count: number): Promise<TerminalStarted[]> {
  const starts: TerminalStarted[] = []
  return new Promise((resolve) => {
    sessions.onStarted((event) => {
      starts.push(event)
      if (starts.length === count) {
        resolve(starts)
      }
    })
  })
}

describe('PtySessions', () => {
  it('starts a shell at the size last asked for while it was opening', async () => {
    const { spawned, spawn } = fakeSpawn()
    const sessions = new PtySessions(spawn)
    const started = nextStarts(sessions, 1)

    sessions.open(startOf('pty-1', { directoryOf: 'pty-0' }))
    sessions.resize('pty-1', { cols: 120, rows: 40 })
    await started

    expect(spawned.map((pty) => [pty.cols, pty.rows])).toEqual([[120, 40]])
  })

  it('types what was typed while a terminal was opening, after its first command, and counts a submitted line', async () => {
    const { written, spawn } = fakeSpawn()
    const sessions = new PtySessions(spawn)
    const started = nextStarts(sessions, 1)
    const submitted: string[] = []
    sessions.onSubmit((id) => {
      submitted.push(id)
    })

    sessions.open(startOf('pty-1', { command: 'claude --resume x\r' }))
    sessions.write('pty-1', 'pw')
    sessions.write('pty-1', 'd\r')
    expect(written).toEqual([])
    await started

    expect(written).toEqual(['claude --resume x\r', 'pw', 'd\r'])
    expect(submitted).toEqual(['pty-1'])
  })

  it('starts a split of a terminal that is itself still opening where that terminal will start', async () => {
    const { spawned, spawn } = fakeSpawn()
    const sessions = new PtySessions(spawn)
    const started = nextStarts(sessions, 2)
    const directory = tmpdir()

    sessions.open(startOf('pty-1', { directory }))
    sessions.open(startOf('pty-2', { directoryOf: 'pty-1' }))
    await started

    expect(spawned.map((pty) => pty.cwd)).toEqual([directory, directory])
  })

  it('spawns nothing for a terminal closed while it was opening, and reports its exit once', async () => {
    const { spawned, spawn } = fakeSpawn()
    const sessions = new PtySessions(spawn)
    const exited = nextExits(sessions, 1)

    sessions.open(startOf('pty-1', { directoryOf: 'pty-0' }))
    sessions.close('pty-1')
    sessions.close('pty-1')
    const exits = await exited

    expect(spawned).toEqual([])
    expect(exits.map((event) => event.id)).toEqual(['pty-1'])
  })

  it('reports an exit for a shell that cannot start', async () => {
    const { spawn } = fakeSpawn(new Error('no such shell'))
    const sessions = new PtySessions(spawn)
    const exited = nextExits(sessions, 1)

    sessions.open(startOf('pty-1'))
    const exits = await exited

    expect(exits).toEqual([{ id: 'pty-1', exitCode: 1 }])
  })

  it('says where a shell started, falling back to the home directory for an unusable one', async () => {
    const { spawn } = fakeSpawn()
    const sessions = new PtySessions(spawn)
    const started = nextStarts(sessions, 2)

    sessions.open(startOf('pty-1', { directory: tmpdir() }))
    sessions.open(startOf('pty-2', { directory: '/no/such/directory' }))
    const starts = await started

    const byId = Object.fromEntries(starts.map((event) => [event.id, event.directory]))
    expect(byId).toEqual({ 'pty-1': tmpdir(), 'pty-2': homedir() })
  })
})
