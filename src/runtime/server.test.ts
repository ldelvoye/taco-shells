import { homedir, tmpdir } from 'node:os'
import { describe, expect, it } from 'vitest'

import type { PtyDataEvent } from '@protocol/ipc'
import type { TerminalStart } from '@protocol/runtime'
import type {
  RuntimeCommand,
  RuntimeReport,
  TerminalInput,
  TerminalOutput
} from '@protocol/runtimeMessages'
import { serveRuntime, type RuntimeServer } from './server'
import { PtySessions, type SpawnedPty, type SpawnPty } from './sessions'

type DataListener = Parameters<SpawnedPty['onData']>[0]
type ExitListener = Parameters<SpawnedPty['onExit']>[0]

interface FakePty {
  pty: SpawnedPty
  written: string[]
  resized: number[][]
  killed: boolean
  fireData(data: string): void
  fireExit(exitCode: number): void
}

function roundTrip<Message>(message: Message): Message {
  return JSON.parse(JSON.stringify(message))
}

function fakePty(): FakePty {
  const dataListeners: DataListener[] = []
  const exitListeners: ExitListener[] = []
  const fake: FakePty = {
    written: [],
    resized: [],
    killed: false,
    fireData(data) {
      for (const listener of dataListeners) {
        listener(data)
      }
    },
    fireExit(exitCode) {
      for (const listener of exitListeners) {
        listener({ exitCode })
      }
    },
    pty: {
      pid: 1,
      write: (data) => {
        fake.written.push(String(data))
      },
      resize: (cols, rows) => {
        fake.resized.push([cols, rows])
      },
      kill: () => {
        fake.killed = true
      },
      onData: (listener) => {
        dataListeners.push(listener)
        return { dispose: () => undefined }
      },
      onExit: (listener) => {
        exitListeners.push(listener)
        return { dispose: () => undefined }
      }
    }
  }
  return fake
}

function startOf(id: string, overrides: Partial<TerminalStart> = {}): TerminalStart {
  return {
    id,
    directory: tmpdir(),
    directoryOf: null,
    command: null,
    size: { cols: 80, rows: 24 },
    ...overrides
  }
}

function served(): {
  sessions: PtySessions
  server: RuntimeServer
  ptys: FakePty[]
  reports: RuntimeReport[]
  send(command: RuntimeCommand): void
  nextReports(count: number): Promise<RuntimeReport[]>
} {
  const ptys: FakePty[] = []
  const spawn: SpawnPty = () => {
    const fake = fakePty()
    ptys.push(fake)
    return fake.pty
  }
  const sessions = new PtySessions(spawn)
  const reports: RuntimeReport[] = []
  const waiters: (() => void)[] = []
  const server = serveRuntime(sessions, (report) => {
    reports.push(roundTrip(report))
    for (const waiter of waiters) {
      waiter()
    }
  })
  return {
    sessions,
    server,
    ptys,
    reports,
    send: (command) => {
      server.receive(roundTrip(command))
    },
    nextReports: (count) => {
      return new Promise((resolve) => {
        const check = (): void => {
          if (reports.length >= count) {
            resolve(reports)
          }
        }
        waiters.push(check)
        check()
      })
    }
  }
}

function nextData(sessions: PtySessions): Promise<PtyDataEvent> {
  return new Promise((resolve) => {
    sessions.onData((event) => {
      resolve(event)
    })
  })
}

function windowOf(server: RuntimeServer): { outputs: TerminalOutput[]; send(input: TerminalInput): void } {
  const outputs: TerminalOutput[] = []
  const input = server.connectWindow((output) => {
    outputs.push(roundTrip(output))
  })
  return {
    outputs,
    send: (message) => {
      input(roundTrip(message))
    }
  }
}

describe('RuntimeServer', () => {
  it('opens, writes to, resizes and closes a terminal from commands', async () => {
    const { send, ptys, nextReports } = served()

    send({ type: 'open', start: startOf('t1') })
    await nextReports(1)
    send({ type: 'write', id: 't1', data: 'ls' })
    send({ type: 'resize', id: 't1', size: { cols: 100, rows: 30 } })
    send({ type: 'close', id: 't1' })

    expect(ptys[0].written).toEqual(['ls'])
    expect(ptys[0].resized).toEqual([[100, 30]])
    expect(ptys[0].killed).toBe(true)
  })

  it('reports a started shell, a submitted line and an exit', async () => {
    const { send, ptys, reports, nextReports } = served()

    send({ type: 'open', start: startOf('t1') })
    await nextReports(1)
    send({ type: 'write', id: 't1', data: 'ls\r' })
    ptys[0].fireExit(3)

    expect(reports).toEqual([
      { type: 'started', event: { id: 't1', directory: tmpdir() } },
      { type: 'submitted', id: 't1' },
      { type: 'exited', event: { id: 't1', exitCode: 3 } }
    ])
  })

  it('answers a query under the number it was asked with', async () => {
    const { send, reports, nextReports } = served()

    send({ type: 'open', start: startOf('t1', { directory: tmpdir() }) })
    send({ type: 'open', start: startOf('t2', { directory: homedir() }) })
    send({ type: 'ask', ask: 7, query: { kind: 'directoryOf', id: 't2' } })
    send({ type: 'ask', ask: 3, query: { kind: 'directoryOf', id: 't1' } })
    await nextReports(4)
    send({ type: 'ask', ask: 9, query: { kind: 'terminalForPids', pids: [1] } })
    await nextReports(5)

    const answers = reports.filter((report) => report.type === 'answer')
    expect(answers).toHaveLength(3)
    expect(answers).toContainEqual({ type: 'answer', ask: 7, value: homedir() })
    expect(answers).toContainEqual({ type: 'answer', ask: 3, value: tmpdir() })
    expect(answers).toContainEqual({ type: 'answer', ask: 9, value: 't1' })
  })

  it('holds output until the window is listening, then sends it in order, the exit last', async () => {
    const { send, sessions, server, ptys, nextReports } = served()

    send({ type: 'open', start: startOf('t1') })
    send({ type: 'open', start: startOf('t2') })
    await nextReports(2)

    const firstData = nextData(sessions)
    ptys[0].fireData('a')
    await firstData
    const output = windowOf(server)
    const secondData = nextData(sessions)
    ptys[1].fireData('b')
    await secondData
    ptys[0].fireExit(0)

    expect(output.outputs).toEqual([])

    output.send({ type: 'listening' })
    expect(output.outputs).toEqual([
      { type: 'data', event: { id: 't1', data: 'a' } },
      { type: 'data', event: { id: 't2', data: 'b' } },
      { type: 'exit', event: { id: 't1', exitCode: 0 } }
    ])

    const thirdData = nextData(sessions)
    ptys[1].fireData('c')
    await thirdData
    expect(output.outputs[3]).toEqual({ type: 'data', event: { id: 't2', data: 'c' } })
  })

  it('takes typing and a new size from the window', async () => {
    const { send, server, ptys, nextReports } = served()
    const page = windowOf(server)

    send({ type: 'open', start: startOf('t1') })
    await nextReports(1)
    page.send({ type: 'write', id: 't1', data: 'ls' })
    page.send({ type: 'resize', id: 't1', size: { cols: 100, rows: 30 } })

    expect(ptys[0].written).toEqual(['ls'])
    expect(ptys[0].resized).toEqual([[100, 30]])
  })

  it('a second window connection replaces the first, and starts by holding output again', async () => {
    const { send, sessions, server, ptys, nextReports } = served()
    const older = windowOf(server)

    send({ type: 'open', start: startOf('t1') })
    await nextReports(1)
    older.send({ type: 'listening' })
    const newer = windowOf(server)

    const data = nextData(sessions)
    ptys[0].fireData('x')
    await data
    older.send({ type: 'listening' })
    older.send({ type: 'write', id: 't1', data: 'stale' })

    expect(older.outputs).toEqual([])
    expect(newer.outputs).toEqual([])
    expect(ptys[0].written).toEqual([])

    newer.send({ type: 'listening' })
    expect(older.outputs).toEqual([])
    expect(newer.outputs).toEqual([{ type: 'data', event: { id: 't1', data: 'x' } }])
  })
})
