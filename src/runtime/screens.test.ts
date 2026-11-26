import { describe, expect, it } from 'vitest'

import type { PtySize, SessionId } from '@protocol/ipc'
import type { TerminalEvent, TerminalRequest } from '@protocol/runtimeMessages'
import type { TerminalColors } from '@protocol/theme'
import type { Screen, ScreenOptions } from './screen'
import { FLOOD_LIMIT, Screens } from './screens'

const SIZE: PtySize = { cols: 80, rows: 24 }
const SMALL: PtySize = { cols: 20, rows: 5 }
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

interface FakeScreen {
  options: ScreenOptions
  calls: string[]
  waiting: (() => void)[]
  fireTitle(title: string): void
  fireReply(data: string): void
  runNext(): void
  runAll(): void
}

interface Harness {
  screens: Screens
  shell: string[]
  events: TerminalEvent[]
  request(request: TerminalRequest): void
  open(id: SessionId): FakeScreen
}

function fakeScreen(options: ScreenOptions): { fake: FakeScreen; screen: Screen } {
  const calls: string[] = []
  const waiting: (() => void)[] = []
  const titleListeners: ((title: string) => void)[] = []
  const replyListeners: ((data: string) => void)[] = []
  let text = ''

  function runNext(): void {
    const next = waiting.shift()
    if (next !== undefined) {
      next()
    }
  }

  const screen: Screen = {
    write: (data, parsed) => {
      waiting.push(() => {
        text += data
        calls.push(`parsed ${data.length}`)
        parsed()
      })
    },
    afterParsed: (then) => {
      waiting.push(then)
    },
    resize: (size) => {
      calls.push(`resize ${size.cols}x${size.rows}`)
    },
    clear: () => {
      calls.push('clear')
    },
    setScrollback: (lines) => {
      calls.push(`scrollback ${lines}`)
    },
    setColors: (colors) => {
      calls.push(`colors ${colors.background}`)
    },
    snapshot: () => text,
    onTitle: (listener) => {
      titleListeners.push(listener)
    },
    onReply: (listener) => {
      replyListeners.push(listener)
    },
    dispose: () => {
      calls.push('dispose')
    }
  }
  const fake: FakeScreen = {
    options,
    calls,
    waiting,
    fireTitle: (title) => {
      for (const listener of titleListeners) {
        listener(title)
      }
    },
    fireReply: (data) => {
      for (const listener of replyListeners) {
        listener(data)
      }
    },
    runNext,
    runAll: () => {
      while (waiting.length > 0) {
        runNext()
      }
    }
  }
  return { fake, screen }
}

function harness(): Harness {
  const made: FakeScreen[] = []
  const shell: string[] = []
  const events: TerminalEvent[] = []
  const screens = new Screens({
    makeScreen: (options) => {
      const { fake, screen } = fakeScreen(options)
      made.push(fake)
      return screen
    },
    shells: {
      write: (id, data) => {
        shell.push(`write ${id} ${data}`)
      },
      resize: (id, size) => {
        shell.push(`resize ${id} ${size.cols}x${size.rows}`)
      },
      pause: (id) => {
        shell.push(`pause ${id}`)
      },
      resume: (id) => {
        shell.push(`resume ${id}`)
      }
    }
  })
  const request = screens.connect((event) => {
    events.push(event)
  })
  return {
    screens,
    shell,
    events,
    request,
    open: (id) => {
      screens.open(id, SIZE)
      return made[made.length - 1]
    }
  }
}

function attached(snapshot: string, size: PtySize = SIZE, attachment = 1): TerminalEvent {
  return { type: 'attached', id: 't1', attachment, size, snapshot }
}

function output(data: string): TerminalEvent {
  return { type: 'output', id: 't1', data }
}

function attachedAndSettled(): Harness & { screen: FakeScreen } {
  const subject = harness()
  const screen = subject.open('t1')
  subject.request({ type: 'attach', id: 't1', size: null, attachment: 1 })
  screen.runAll()
  subject.events.length = 0
  return { ...subject, screen }
}

describe('Screens', () => {
  it('makes a screen with the current scrollback and colors, and new ones reach the screens that exist', () => {
    const { screens, open } = harness()

    const first = open('t1')
    screens.setScrollback(50)
    screens.setColors(COLORS)
    const second = open('t2')

    expect(first.options).toEqual({ size: SIZE, scrollback: 1000 })
    expect(second.options).toEqual({ size: SIZE, scrollback: 50 })
    expect(first.calls).toEqual(['scrollback 50', 'colors #123456'])
    expect(second.calls).toEqual(['colors #123456'])
  })

  it('sends output to a view as it arrives, keeps it in order while the view attaches, and not at all with no view', () => {
    const { screens, request, open, events, shell } = harness()
    const screen = open('t1')

    screens.output('t1', 'a')
    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    screens.output('t1', 'b')
    screens.output('t1', 'c')
    expect(events).toEqual([])

    screen.runNext()
    screen.runNext()
    expect(events).toEqual([attached('a'), output('b'), output('c')])

    screens.output('t1', 'd')
    expect(events[3]).toEqual(output('d'))
    request({ type: 'write', id: 't1', data: 'ls' })
    expect(shell).toEqual(['write t1 ls'])
    screen.runAll()
    expect(screen.calls).toEqual(['parsed 1', 'parsed 1', 'parsed 1', 'parsed 1'])

    request({ type: 'detach', id: 't1' })
    screens.output('t1', 'e')
    expect(events).toHaveLength(4)
  })

  it('resizes first for an attach that carries another size, without telling that view; the same size does nothing', () => {
    const { request, open, events, shell } = harness()
    const screen = open('t1')
    const wider: PtySize = { cols: 100, rows: 30 }

    request({ type: 'attach', id: 't1', size: wider, attachment: 1 })
    screen.runAll()
    request({ type: 'attach', id: 't1', size: wider, attachment: 1 })
    screen.runAll()

    expect(shell).toEqual(['resize t1 100x30'])
    expect(screen.calls).toEqual(['resize 100x30'])
    expect(events).toEqual([attached('', wider), attached('', wider)])

    request({ type: 'attach', id: 't1', size: SMALL, attachment: 2 })
    screen.runAll()
    expect(events).toEqual([attached('', wider), attached('', wider), attached('', SMALL, 2)])
  })

  it('posts nothing for an attach cancelled by a detach, a second attach or a new connection', () => {
    const { screens, request, open, events, shell } = harness()
    const screen = open('t1')

    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    request({ type: 'detach', id: 't1' })
    screen.runAll()
    expect(events).toEqual([])

    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    screen.runAll()
    expect(events).toEqual([attached('')])

    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    const newer: TerminalEvent[] = []
    const newerRequest = screens.connect((event) => {
      newer.push(event)
    })
    screen.runAll()
    request({ type: 'write', id: 't1', data: 'stale' })
    expect(events).toHaveLength(1)
    expect(newer).toEqual([])
    expect(shell).toEqual([])

    newerRequest({ type: 'attach', id: 't1', size: null, attachment: 1 })
    screen.runAll()
    expect(newer).toEqual([attached('')])

    newerRequest({ type: 'attach', id: 'gone', size: null, attachment: 1 })
    expect(newer).toHaveLength(1)
  })

  it('resizes the shell at once, the screen and the view at the request own place among the output', () => {
    const { screens, request, screen, events, shell } = attachedAndSettled()

    screens.output('t1', 'a')
    request({ type: 'resize', id: 't1', size: SMALL })
    screens.output('t1', 'b')

    expect(shell).toEqual(['resize t1 20x5'])
    expect(screen.calls).toEqual([])
    expect(events).toEqual([output('a'), { type: 'resized', id: 't1', size: SMALL }, output('b')])

    screen.runAll()
    expect(screen.calls).toEqual(['parsed 1', 'resize 20x5', 'parsed 1'])

    request({ type: 'resize', id: 't1', size: SMALL })
    expect(shell).toEqual(['resize t1 20x5'])
    expect(events).toHaveLength(3)
    expect(screen.waiting).toEqual([])
  })

  it('attaches with the size its snapshot has when a resize is asked for while it attaches', () => {
    const { screens, request, open, events } = harness()
    const screen = open('t1')

    screens.output('t1', 'a')
    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    request({ type: 'resize', id: 't1', size: SMALL })
    screen.runAll()

    expect(events).toEqual([attached('a', SIZE), { type: 'resized', id: 't1', size: SMALL }])
  })

  it('clears the screen at its place in the output and tells the view at the request', () => {
    const { screens, request, screen, events } = attachedAndSettled()

    screens.output('t1', 'a')
    request({ type: 'clear', id: 't1' })
    screens.output('t1', 'b')

    expect(events).toEqual([output('a'), { type: 'cleared', id: 't1' }, output('b')])
    screen.runAll()
    expect(screen.calls).toEqual(['parsed 1', 'clear', 'parsed 1'])
  })

  it('ends an attached view after the output before it, then lets the screen go', () => {
    const { screens, request, screen, events } = attachedAndSettled()

    screens.output('t1', 'a')
    screens.ended('t1', 3)
    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    expect(events).toEqual([output('a')])
    expect(screen.waiting).toHaveLength(2)

    screen.runAll()
    expect(events).toEqual([output('a'), { type: 'end', id: 't1', exitCode: 3 }])
    expect(screen.calls).toEqual(['parsed 1', 'dispose'])

    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    expect(screen.waiting).toEqual([])
  })

  it('ends a terminal that exits while a view attaches after the snapshot, and one with no view silently', () => {
    const { screens, request, open, events } = harness()
    const attaching = open('t1')
    const unseen = open('t2')

    request({ type: 'attach', id: 't1', size: null, attachment: 1 })
    screens.ended('t1', 1)
    screens.ended('t2', 0)
    attaching.runAll()
    unseen.runAll()

    expect(events).toEqual([attached(''), { type: 'end', id: 't1', exitCode: 1 }])
    expect(unseen.calls).toEqual(['dispose'])
  })

  it('pauses the shell once above the flood limit and resumes it when everything is parsed', () => {
    const { screens, open, shell } = harness()
    const screen = open('t1')
    const half = 'x'.repeat(FLOOD_LIMIT / 2 + 1)

    screens.output('t1', half)
    expect(shell).toEqual([])
    screens.output('t1', half)
    screens.output('t1', 'y')
    expect(shell).toEqual(['pause t1'])

    screen.runNext()
    expect(shell).toEqual(['pause t1'])
    screen.runAll()
    expect(shell).toEqual(['pause t1', 'resume t1'])
  })

  it('does not resume a terminal that ends while paused', () => {
    const { screens, open, shell } = harness()
    const screen = open('t1')
    const half = 'x'.repeat(FLOOD_LIMIT / 2 + 1)

    screens.output('t1', half)
    screens.output('t1', half)
    screens.ended('t1', 0)
    screen.runAll()

    expect(shell).toEqual(['pause t1'])
  })

  it("reports a screen's title with the terminal's id, and sends its answers to that terminal's shell", () => {
    const { screens, shell, open } = harness()
    const titles: string[][] = []
    screens.onTitle((id, title) => {
      titles.push([id, title])
    })

    const screen = open('t1')
    screen.fireTitle('Tacos')
    screen.fireReply('\x1b[0n')

    expect(titles).toEqual([['t1', 'Tacos']])
    expect(shell).toEqual(['write t1 \x1b[0n'])
  })
})
