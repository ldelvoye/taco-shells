import { Terminal } from '@xterm/headless'
import { describe, expect, it } from 'vitest'

import { makeXtermScreen } from '@runtime/engine/xtermScreen'
import { trackCursorHidden } from '@runtime/engine/cursor'
import type { Screen } from '@runtime/screen'
import {
  clearInOrder,
  leaveAnswersToTheRuntime,
  loadSnapshot,
  resizeInOrder
} from '@window-engine/stream'

import { CASES } from './cases'
import type { Case } from './cases'
import { describeTerminal } from './describe'

const SIZE = { cols: 80, rows: 24 }
const SCROLLBACK = 1000

function makeScreen(): Screen {
  return makeXtermScreen({ size: SIZE, scrollback: SCROLLBACK })
}

function makeTerminal(size = SIZE): { term: Terminal; cursorHidden: () => boolean } {
  const term = new Terminal({
    cols: size.cols,
    rows: size.rows,
    scrollback: SCROLLBACK,
    allowProposedApi: true
  })
  const cursorHidden = trackCursorHidden(term)
  return { term, cursorHidden }
}

function written(term: Terminal, data: string): Promise<void> {
  return new Promise((resolve) => {
    term.write(data, resolve)
  })
}

function parsed(screen: Screen, data: string): Promise<void> {
  return new Promise((resolve) => {
    screen.write(data, resolve)
  })
}

function snapshotOf(screen: Screen): Promise<string> {
  return new Promise((resolve) => {
    screen.afterParsed(() => {
      resolve(screen.snapshot())
    })
  })
}

async function loaded(snapshot: string, size = SIZE): Promise<Terminal> {
  const view = makeTerminal(size)
  loadSnapshot(view.term, snapshot, size)
  await written(view.term, '')
  return view.term
}

async function outcomeOf(testCase: Case): Promise<void> {
  const screen = makeScreen()
  screen.write(testCase.before, () => {})
  const snapshot = await snapshotOf(screen)

  const view = makeTerminal()
  loadSnapshot(view.term, snapshot, SIZE)
  await written(view.term, testCase.after)

  const reference = makeTerminal()
  await written(reference.term, testCase.before)
  await written(reference.term, testCase.after)

  const viewed = describeTerminal(view.term, view.cursorHidden())
  const expected = describeTerminal(reference.term, reference.cursorHidden())
  expect(viewed).toEqual(expected)
  screen.dispose()
}

describe('a snapshot carries what the screen shows', () => {
  for (const testCase of CASES) {
    if (testCase.known === undefined) {
      it(testCase.name, async () => {
        await outcomeOf(testCase)
      })
    } else {
      it.fails(`${testCase.name} (${testCase.known})`, async () => {
        await outcomeOf(testCase)
      })
    }
  }
})

describe('the runtime screen', () => {
  it('a resize placed with afterParsed lands after the output written before it', async () => {
    const screen = makeXtermScreen({ size: { cols: 10, rows: 5 }, scrollback: SCROLLBACK })
    const letters = 'abcdefghijklmno'
    screen.write(letters, () => {})
    screen.afterParsed(() => {
      screen.resize({ cols: 20, rows: 5 })
    })
    const snapshot = await snapshotOf(screen)

    const view = await loaded(snapshot, { cols: 20, rows: 5 })
    const lines = describeTerminal(view, false).normal
    const firstTwo = lines.slice(0, 2)
    // Written at 10 columns the letters take two lines. Written after the resize they would take one.
    expect(firstTwo).toEqual(['abcdefghij', 'klmno'])
  })

  it('a view that places a resize and a clear in its output ends up as the screen does', async () => {
    const narrow = { cols: 10, rows: 5 }
    const wide = { cols: 20, rows: 5 }
    const screen = makeXtermScreen({ size: narrow, scrollback: SCROLLBACK })
    const view = makeTerminal(narrow)

    screen.write('abcdefghijklmno', () => {})
    view.term.write('abcdefghijklmno')
    screen.afterParsed(() => {
      screen.resize(wide)
    })
    resizeInOrder(view.term, wide)
    screen.write('\r\nsecond\r\n', () => {})
    view.term.write('\r\nsecond\r\n')
    screen.afterParsed(() => {
      screen.clear()
    })
    clearInOrder(view.term)
    screen.write('third', () => {})
    view.term.write('third')

    const snapshot = await snapshotOf(screen)
    const fresh = await loaded(snapshot, wide)
    await written(view.term, '')
    expect(describeTerminal(view.term, false)).toEqual(describeTerminal(fresh, false))
    expect(view.term.cols).toBe(20)
  })

  it('answers what a program asks the terminal, and a view leaves each of those to it', async () => {
    const questions = [
      '\x1b[c',
      '\x1b[>c',
      '\x1b[5n',
      '\x1b[6n',
      '\x1b[?6n',
      '\x1b[4$p',
      '\x1b[?25$p',
      '\x1bP$qr\x1b\\'
    ]
    for (const question of questions) {
      const screen = makeScreen()
      const answers: string[] = []
      screen.onReply((data) => {
        answers.push(data)
      })
      await parsed(screen, question)

      const view = makeTerminal()
      const fromTheView: string[] = []
      leaveAnswersToTheRuntime(view.term)
      view.term.onData((data) => {
        fromTheView.push(data)
      })
      await written(view.term, question)

      expect(answers, JSON.stringify(question)).toHaveLength(1)
      expect(fromTheView, JSON.stringify(question)).toEqual([])
    }
  })

  it('reports the title a program sets, the last one winning', async () => {
    const screen = makeScreen()
    const titles: string[] = []
    screen.onTitle((title) => {
      titles.push(title)
    })
    await parsed(screen, '\x1b]0;zero\x07\x1b]1;one\x07\x1b]2;two\x07')
    expect(titles).toEqual(['zero', 'one', 'two'])
  })

  it('clear and a smaller scrollback take lines away', async () => {
    const small = { cols: 80, rows: 5 }
    const screen = makeXtermScreen({ size: small, scrollback: SCROLLBACK })
    const output: string[] = []
    for (let n = 1; n <= 50; n += 1) {
      output.push(`old ${n}`)
    }
    await parsed(screen, output.join('\r\n'))

    screen.setScrollback(10)
    const trimmedSnapshot = await snapshotOf(screen)
    const trimmed = await loaded(trimmedSnapshot, small)
    const afterTrim = describeTerminal(trimmed, false).normal
    expect(afterTrim[0]).toBe('old 36')
    expect(afterTrim).toHaveLength(15)

    screen.clear()
    const clearedSnapshot = await snapshotOf(screen)
    const cleared = await loaded(clearedSnapshot, small)
    const afterClear = describeTerminal(cleared, false).normal
    const kept = afterClear.filter((line) => line.startsWith('old'))
    expect(kept).toEqual(['old 50'])
  })
})
