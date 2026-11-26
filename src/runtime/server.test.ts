import { homedir, tmpdir } from 'node:os'
import { describe, expect, it } from 'vitest'

import type { TerminalStart } from '@protocol/runtime'
import type {
  RuntimeCommand,
  RuntimeReport,
  TerminalEvent,
  TerminalRequest
} from '@protocol/runtimeMessages'
import type { TerminalColors } from '@protocol/theme'
import type { Screen } from './screen'
import { Screens } from './screens'
import { serveRuntime, type RuntimeServer } from './server'
import { PtySessions, type SpawnedPty, type SpawnPty } from './sessions'

const COLORS: TerminalColors = {
  background: '#123456',
  foreground: '#010101',
  cursor: '#020202',
  cursorAccent: '#030303',
  selectionBackground: '#040404',
  black: '#050505',
  red: '#060606',
  green: '#070707',
  yellow: '#080808',
  blue: '#090909',
  magenta: '#0a0a0a',
  cyan: '#0b0b0b',
  white: '#0c0c0c',
  brightBlack: '#0d0d0d',
  brightRed: '#0e0e0e',
  brightGreen: '#0f0f0f',
  brightYellow: '#101010',
  brightBlue: '#111111',
  brightMagenta: '#121212',
  brightCyan: '#131313',
  brightWhite: '#141414'
}

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
      pause: () => undefined,
      resume: () => undefined,
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

interface StubScreen {
  screen: Screen
  scrollbacks: number[]
  colors: TerminalColors[]
  fireTitle(title: string): void
}

function stubScreen(): StubScreen {
  const scrollbacks: number[] = []
  const colors: TerminalColors[] = []
  const titleListeners: ((title: string) => void)[] = []
  const screen: Screen = {
    write: (_data, parsed) => {
      parsed()
    },
    afterParsed: (then) => {
      then()
    },
    resize: () => undefined,
    clear: () => undefined,
    setScrollback: (lines) => {
      scrollbacks.push(lines)
    },
    setColors: (next) => {
      colors.push(next)
    },
    snapshot: () => '',
    onTitle: (listener) => {
      titleListeners.push(listener)
    },
    onReply: () => undefined,
    dispose: () => undefined
  }
  return {
    screen,
    scrollbacks,
    colors,
    fireTitle(title) {
      for (const listener of titleListeners) {
        listener(title)
      }
    }
  }
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
  stubs: StubScreen[]
  reports: RuntimeReport[]
  send(command: RuntimeCommand): void
  nextReports(count: number): Promise<RuntimeReport[]>
} {
  const ptys: FakePty[] = []
  const stubs: StubScreen[] = []
  const spawn: SpawnPty = () => {
    const fake = fakePty()
    ptys.push(fake)
    return fake.pty
  }
  const sessions = new PtySessions(spawn)
  const reports: RuntimeReport[] = []
  const waiters: (() => void)[] = []
  const screens = new Screens({
    makeScreen: () => {
      const stub = stubScreen()
      stubs.push(stub)
      return stub.screen
    },
    shells: sessions
  })
  const server = serveRuntime(sessions, screens, (report) => {
    reports.push(roundTrip(report))
    for (const waiter of waiters) {
      waiter()
    }
  })
  return {
    sessions,
    server,
    ptys,
    stubs,
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

function windowOf(server: RuntimeServer): { events: TerminalEvent[]; send(request: TerminalRequest): void } {
  const events: TerminalEvent[] = []
  const request = server.connectWindow((event) => {
    events.push(roundTrip(event))
  })
  return {
    events,
    send: (message) => {
      request(roundTrip(message))
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

  it("feeds a shell's output and exit to its screen, and reports its titles, scrollback and colors", async () => {
    const { send, server, ptys, stubs, reports, nextReports } = served()
    const page = windowOf(server)

    send({ type: 'open', start: startOf('t1') })
    await nextReports(1)
    page.send({ type: 'attach', id: 't1', size: null, attachment: 1 })
    ptys[0].fireData('a')
    stubs[0].fireTitle('Tacos')
    send({ type: 'scrollback', lines: 500 })
    send({ type: 'colors', colors: COLORS })
    ptys[0].fireExit(2)

    expect(page.events).toEqual([
      { type: 'attached', id: 't1', attachment: 1, size: { cols: 80, rows: 24 }, snapshot: '' },
      { type: 'output', id: 't1', data: 'a' },
      { type: 'end', id: 't1', exitCode: 2 }
    ])
    expect(reports).toContainEqual({ type: 'titled', id: 't1', title: 'Tacos' })
    expect(stubs[0].scrollbacks).toEqual([500])
    expect(stubs[0].colors).toEqual([COLORS])
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
})
