import { existsSync, mkdtempSync, readFileSync, realpathSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, KEY, launchApp } from './app'

// Covers what the unit tests cannot: a chord reaching its command, and the
// command reaching the screen. Model decisions live in shared/model.test.ts.

function reportedDirectory(file: string): string | null {
  if (!existsSync(file)) {
    return null
  }

  const reported = readFileSync(file, 'utf8')
  return reported.trim()
}

let app: App

beforeEach(async () => {
  app = await launchApp()
})

afterEach(async () => {
  await app.close()
})

describe('the default keymap', () => {
  it('splits the active terminal on cmd+backslash', async () => {
    await app.press(KEY.backslash, { cmd: true })

    const screen = await app.until('the group holds two panes', (state) => state.rows.length === 2)

    expect(screen.groups).toBe(1)
    expect(screen.dividers).toBe(1)
    expect(screen.rows.map((row) => row.connector)).toEqual(['is-first', 'is-last'])
    expect(screen.panes.map((pane) => pane.width)).toEqual([0.5, 0.5])
    expect(screen.rows.map((row) => row.active)).toEqual([false, true])
  })

  it('opens a separate group on cmd+t rather than splitting', async () => {
    await app.press(KEY.t, { cmd: true })

    const screen = await app.until('a second group exists', (state) => state.groups === 2)

    expect(screen.rows).toHaveLength(2)
    expect(screen.rows.map((row) => row.connector)).toEqual(['', ''])
    expect(screen.dividers).toBe(0)
  })

  it('moves focus between panes on cmd+opt+arrow, wrapping at the ends', async () => {
    await app.press(KEY.backslash, { cmd: true })
    await app.until('the group holds two panes', (state) => state.rows.length === 2)

    await app.press(KEY.right, { cmd: true, alt: true })
    const afterOne = await app.until('the first pane is active', (state) => state.rows[0].active)
    expect(afterOne.rows.map((row) => row.active)).toEqual([true, false])

    await app.press(KEY.right, { cmd: true, alt: true })
    const wrapped = await app.until('focus wraps back', (state) => state.rows[1].active)
    expect(wrapped.rows.map((row) => row.active)).toEqual([false, true])

    await app.press(KEY.left, { cmd: true, alt: true })
    const backwards = await app.until('focus steps back', (state) => state.rows[0].active)
    expect(backwards.rows.map((row) => row.active)).toEqual([true, false])
  })

  it('moves between terminals on cmd+shift+bracket, skipping over a split as one stop', async () => {
    await app.press(KEY.backslash, { cmd: true })
    await app.until('the first terminal is split', (state) => state.rows.length === 2)

    await app.press(KEY.t, { cmd: true })
    await app.until(
      'a second terminal exists and has the keyboard',
      (state) => state.rows.length === 3 && state.rows[2].active
    )

    // Three rows, two terminals: the split's two panes are one stop, so this
    // wraps from the second terminal straight back to the first.
    await app.press(KEY.bracketRight, { cmd: true, shift: true })
    const wrapped = await app.until('focus is back in the split', (state) => state.rows[1].active)
    expect(wrapped.rows.map((row) => row.active)).toEqual([false, true, false])

    await app.press(KEY.bracketLeft, { cmd: true, shift: true })
    const back = await app.until('focus returns to the second terminal', (state) =>
      state.rows[2].active
    )
    expect(back.rows.map((row) => row.active)).toEqual([false, false, true])
  })

  it('hides and shows the sidebar on cmd+b', async () => {
    await app.press(KEY.b, { cmd: true })
    await app.until('the sidebar is gone', (state) => !state.sidebarVisible)

    await app.press(KEY.b, { cmd: true })
    await app.until('the sidebar is back', (state) => state.sidebarVisible)
  })

  it('closes the focused pane on cmd+w, collapsing the group back to one row', async () => {
    await app.press(KEY.backslash, { cmd: true })
    await app.until('the group holds two panes', (state) => state.rows.length === 2)

    await app.press(KEY.w, { cmd: true })

    const screen = await app.until('one pane is left', (state) => state.rows.length === 1)
    expect(screen.dividers).toBe(0)
    expect(screen.rows[0].connector).toBe('')
    expect(screen.panes.map((pane) => pane.width)).toEqual([1])
  })

  it('sends esc+return on shift+enter, and only that', async () => {
    const directory = mkdtempSync(join(tmpdir(), 'taco-shells-newline-'))
    const ready = join(directory, 'ready')
    const received = join(directory, 'received')
    const surplus = join(directory, 'surplus')

    // The shell is the only witness to what left the terminal: it reports the
    // bytes of its line, then reads once more so a second line would show up too.
    // The report is renamed into place, so the file waited on is never half-written.
    const write = `printf '%s' "$line" | od -An -tx1 > ${received}.part`
    const report = `${write}; mv ${received}.part ${received}`
    const script = `printf ready > ${ready}; read -r line; ${report}; read -r; printf seen > ${surplus}`
    await app.runInPane('pty-1', script)
    await app.until('the shell is reading', () => existsSync(ready))

    await app.press(KEY.enter, { shift: true })
    await app.until('the line arrives', () => existsSync(received))

    // ESC alone: the return that followed it ended the line rather than joining
    // it. A bare CR, which is all xterm sends for shift+enter, reads as an empty
    // line here and submits whatever was typed in real use.
    expect(readFileSync(received, 'utf8').trim()).toBe('1b')

    await app.pause(1000)
    expect(existsSync(surplus)).toBe(false)
  })
})

describe('a split', () => {
  it('leaves every pane in the group the same width, however many times it is split', async () => {
    let splits = 0
    while (splits < 4) {
      await app.press(KEY.backslash, { cmd: true })
      await app.until('the split lands', (state) => state.rows.length === splits + 2)
      splits += 1
    }

    const screen = await app.screen()
    const widths = screen.panes.map((pane) => pane.pixels)
    const narrowest = Math.min(...widths)
    const widest = Math.max(...widths)

    // Halving the pane that was split instead would leave the last pane a
    // sixteenth of the first by this point.
    expect(widest - narrowest).toBeLessThanOrEqual(1)
  })

  it('is refused once the panes would be too narrow, leaving no shell behind', async () => {
    let attempts = 0
    while (attempts < 10) {
      await app.press(KEY.backslash, { cmd: true })
      await app.pause(500)
      attempts += 1
    }

    const screen = await app.screen()
    const narrowest = Math.min(...screen.panes.map((pane) => pane.pixels))
    expect(narrowest).toBeGreaterThan(40)

    // A refused split must not spawn the shell it would have needed: that shell
    // has no pane to appear in and nothing would ever close it.
    expect(app.shellCount()).toBe(screen.rows.length)
  })

  it('ignores the keystrokes a held key repeats on its own', async () => {
    await app.pressAutoRepeat(KEY.backslash, { cmd: true })
    await app.pause(1500)

    const unchanged = await app.screen()
    expect(unchanged.rows).toHaveLength(1)

    // The same key without the repeat flag does split, so the assertion above is
    // about the flag rather than about the keystroke never arriving.
    await app.press(KEY.backslash, { cmd: true })
    await app.until('the deliberate press splits', (state) => state.rows.length === 2)
  })

  it('starts its shell in the directory the split terminal is in', async () => {
    // Each shell is asked to write where it is, rather than the sidebar row
    // being read for it: a row only carries a directory if the machine's shell
    // configuration publishes one as the window title, and most do not. The real
    // path, because the app resolves a pty's directory through lsof, which
    // reports the physical one.
    const directory = realpathSync(mkdtempSync(join(tmpdir(), 'taco-shells-cwd-')))
    const donorReport = join(directory, 'donor')
    const splitReport = join(directory, 'split')

    await app.runInPane('pty-1', `cd ${directory} && pwd > ${donorReport}`)
    await app.until('the first shell has moved', () => reportedDirectory(donorReport) === directory)

    await app.press(KEY.backslash, { cmd: true })
    await app.until('the group holds two panes', (state) => state.rows.length === 2)

    await app.runInPane('pty-2', `pwd > ${splitReport}`)
    await app.until(
      'the new shell has reported its directory',
      () => reportedDirectory(splitReport) === directory
    )
  })

  it('resizes when the divider between two panes is dragged', async () => {
    await app.press(KEY.backslash, { cmd: true })
    const split = await app.until('the group holds two panes', (state) => state.rows.length === 2)
    const startingWidth = split.panes[0].pixels

    await app.dragDivider(0, -120)

    const screen = await app.until(
      'the left pane has narrowed',
      (state) => state.panes[0].pixels < startingWidth - 100
    )

    const widths = screen.panes.map((pane) => pane.width)
    const total = widths[0] + widths[1]
    expect(total).toBeCloseTo(1)
    expect(widths[0]).toBeLessThan(widths[1])
  })
})

describe('the app', () => {
  it('reports nothing to the console while splitting and closing panes', async () => {
    await app.press(KEY.backslash, { cmd: true })
    await app.until('the group holds two panes', (state) => state.rows.length === 2)
    await app.press(KEY.w, { cmd: true })
    await app.until('one pane is left', (state) => state.rows.length === 1)

    expect(app.rendererConsole()).toEqual([])
  })
})
