import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, launchApp } from './app'
import { evaluate, evaluateInMain } from './harness/connection'
import { KEY } from './harness/keys'
import { pollFor } from './harness/poll'
import { untilShown } from './harness/shownText'

const ANSWER_TIMEOUT_MS = 8000
const WARM_VIEWS = 8
const TERMINALS = 10
const NARROWER = { width: 800, height: 600 }

const TWO_SAVED_TERMINALS = {
  version: 1,
  groups: [
    { panes: [{ width: 1, cwd: '/private/tmp', claude: null }], activePane: 0 },
    { panes: [{ width: 1, cwd: '/private/tmp', claude: null }], activePane: 0 }
  ],
  activeGroup: 1
}

// Asks the terminal for its background color and keeps what it answers. The
// answer is 25 bytes, and a terminal that says nothing is given two seconds.
const ASK_FOR_BACKGROUND = `#!/bin/sh
saved=$(stty -g)
stty raw -echo min 0 time 20
printf '\\033]11;?\\033\\\\'
answer=$(dd bs=1 count=25 2>/dev/null)
stty "$saved"
printf '%s' "$answer" > "$1"
`
const BACKGROUND_ANSWER = /\]11;rgb:[0-9a-f]{4}\/[0-9a-f]{4}\/[0-9a-f]{4}/

interface GridSize {
  rows: number
  cols: number
}

let scratch: string
let app: App | null = null
let asked = 0

beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), 'taco-shells-views-'))
})

afterEach(async () => {
  if (app !== null) {
    await app.close()
    app = null
  }
  rmSync(scratch, { recursive: true, force: true })
})

async function fileWrittenBy(running: App, pane: string, command: (file: string) => string): Promise<string> {
  asked += 1
  const file = join(scratch, `answer-${asked}`)
  await running.runInPane(pane, command(file))
  return pollFor(
    `an answer from ${pane}`,
    async () => {
      try {
        const text = readFileSync(file, 'utf8')
        if (text === '') {
          return null
        }
        return text
      } catch {
        return null
      }
    },
    ANSWER_TIMEOUT_MS
  )
}

async function sizeOf(running: App, pane: string): Promise<GridSize> {
  const reported = await fileWrittenBy(running, pane, (file) => `stty size > '${file}'`)
  const parts = reported.trim().split(' ')
  const [rows, cols] = parts.map(Number)
  return { rows, cols }
}

async function untilSameSize(running: App, pane: string, other: string): Promise<GridSize> {
  return pollFor(
    `${pane} to have the size of ${other}`,
    async () => {
      const mine = await sizeOf(running, pane)
      const theirs = await sizeOf(running, other)
      if (mine.cols === theirs.cols && mine.rows === theirs.rows) {
        return mine
      }
      return null
    },
    ANSWER_TIMEOUT_MS
  )
}

async function viewCount(running: App): Promise<number> {
  const counted = await evaluate(running.page, `String(document.querySelectorAll('.terminal-host').length)`)
  return Number(counted)
}

// The window keeps a view only for the terminals on screen and the few shown
// last. Only the real app shows that a terminal without one still has its pane's
// size, still answers a program, and comes back with what it printed.

describe('a terminal the window has no view of', () => {
  it('has the size of its pane and answers a program that asks for a color, without ever having been shown', async () => {
    const configDir = join(scratch, 'config')
    mkdirSync(configDir)
    const saved = JSON.stringify(TWO_SAVED_TERMINALS)
    writeFileSync(join(configDir, 'workspace.json'), saved)
    const script = join(scratch, 'ask.sh')
    writeFileSync(script, ASK_FOR_BACKGROUND)
    chmodSync(script, 0o755)

    app = await launchApp({ configDir, expectedRows: 2 })
    expect(await viewCount(app)).toBe(1)

    const hidden = await untilSameSize(app, 'pty-1', 'pty-2')
    expect(hidden.cols).toBeGreaterThan(80)

    const answer = await fileWrittenBy(app, 'pty-1', (file) => `'${script}' '${file}'`)
    expect(answer).toMatch(BACKGROUND_ANSWER)
    expect(await viewCount(app)).toBe(1)
  })

  it('follows a window resize, and shows what it printed when it is brought back', async () => {
    app = await launchApp({ mainInspector: true })
    for (let opened = 2; opened <= TERMINALS; opened += 1) {
      await app.press(KEY.t, { cmd: true })
      await app.until(`terminal ${opened} is open`, (state) => state.rows.length === opened)
    }
    expect(await viewCount(app)).toBe(WARM_VIEWS)

    const before = await untilSameSize(app, 'pty-1', `pty-${TERMINALS}`)
    await evaluateInMain(`(() => {
      const { BrowserWindow } = process.mainModule.require('electron')
      BrowserWindow.getAllWindows()[0].setContentSize(${NARROWER.width}, ${NARROWER.height})
    })()`)
    const after = await pollFor(
      'the terminal with no view to be told the narrower size',
      async () => {
        const size = await untilSameSize(app as App, 'pty-1', `pty-${TERMINALS}`)
        if (size.cols < before.cols) {
          return size
        }
        return null
      },
      ANSWER_TIMEOUT_MS
    )
    expect(after.rows).toBeLessThan(before.rows)

    await app.runInPane('pty-1', 'echo back-$((40 + 2))')
    for (let step = 1; step < TERMINALS; step += 1) {
      await app.press(KEY.bracketLeft, { cmd: true, shift: true })
    }
    await app.until('the first terminal is on screen', (state) => state.rows[0].active)
    await untilShown(app.page, 'the first terminal to show what it printed', ([text]) =>
      text.includes('back-42')
    )
    expect(await viewCount(app)).toBe(WARM_VIEWS)
  })
})
