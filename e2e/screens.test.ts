import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, launchApp } from './app'
import { KEY } from './harness/keys'
import { pollFor } from './harness/poll'
import { untilShown } from './harness/shownText'

const SIZE_TIMEOUT_MS = 8000

// Each command prints a word that is not in the command itself, so finding the
// word on screen means the command's output is there and not only its echo.
const FIRST = { command: 'echo first-$((40 + 2))', printed: 'first-42' }
const SECOND = { command: 'echo second-$((40 + 3))', printed: 'second-43' }

interface GridSize {
  rows: number
  cols: number
}

let app: App
let scratch: string

beforeEach(async () => {
  scratch = mkdtempSync(join(tmpdir(), 'taco-shells-screens-'))
  app = await launchApp()
})

afterEach(async () => {
  await app.close()
  rmSync(scratch, { recursive: true, force: true })
})

async function reload(rows: number): Promise<void> {
  await app.cdp('Page.reload')
  await app.until('the reloaded window has its rows', (state) => state.rows.length === rows)
}

async function sizeOf(pane: string, name: string): Promise<GridSize> {
  const file = join(scratch, name)
  await app.runInPane(pane, `stty size > '${file}'`)
  const reported = await pollFor(
    `the size of ${pane}`,
    async () => {
      try {
        const text = readFileSync(file, 'utf8').trim()
        if (text === '') {
          return null
        }
        return text
      } catch {
        return null
      }
    },
    SIZE_TIMEOUT_MS
  )
  const [rows, cols] = reported.split(' ').map(Number)
  return { rows, cols }
}

// The runtime keeps each terminal's screen, and a window's view is loaded from
// it. Only the real app shows that a view built afresh has what the last one had.

describe("a terminal's screen outlives the view of it", () => {
  it('a reloaded window shows what its terminals showed', async () => {
    await app.runInPane('pty-1', FIRST.command)
    await app.press(KEY.t, { cmd: true })
    await app.until('the second terminal is open', (state) => state.rows.length === 2)
    await app.runInPane('pty-2', SECOND.command)
    await untilShown(app.page, 'the second terminal to print', ([text]) =>
      text.includes(SECOND.printed)
    )

    await reload(2)

    await untilShown(app.page, 'the second terminal to show its output again', ([text]) =>
      text.includes(SECOND.printed)
    )
    await app.press(KEY.bracketLeft, { cmd: true, shift: true })
    await untilShown(app.page, 'the first terminal to show its output again', ([text]) =>
      text.includes(FIRST.printed)
    )
  })

  it('a cleared terminal is still clear after a reload', async () => {
    await app.runInPane('pty-1', FIRST.command)
    await untilShown(app.page, 'the terminal to print', ([text]) => text.includes(FIRST.printed))

    await app.press(KEY.k, { cmd: true })
    await untilShown(app.page, 'the terminal to be cleared', ([text]) => !text.includes(FIRST.printed))
    await app.runInPane('pty-1', SECOND.command)
    await untilShown(app.page, 'the terminal to print again', ([text]) =>
      text.includes(SECOND.printed)
    )

    await reload(1)

    const [shown] = await untilShown(app.page, 'the terminal to show its output again', ([text]) =>
      text.includes(SECOND.printed)
    )
    expect(shown).not.toContain(FIRST.printed)
  })

  it('a shell has the size of its pane, before and after a split', async () => {
    const whole = await sizeOf('pty-1', 'whole')

    await app.press(KEY.backslash, { cmd: true })
    await app.until('the group holds two panes', (state) => state.rows.length === 2)

    const halved = await pollFor(
      'the first shell to be told its narrower size',
      async () => {
        const size = await sizeOf('pty-1', `halved-${Date.now()}`)
        if (size.cols < whole.cols) {
          return size
        }
        return null
      },
      SIZE_TIMEOUT_MS
    )
    const beside = await sizeOf('pty-2', 'beside')

    expect(whole.cols).toBeGreaterThan(80)
    expect(halved.cols).toBeLessThan(whole.cols * 0.6)
    expect(halved.rows).toBe(whole.rows)
    expect(Math.abs(beside.cols - halved.cols)).toBeLessThanOrEqual(1)
  })
})
