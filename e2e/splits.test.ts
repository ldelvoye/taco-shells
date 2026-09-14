import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, KEY, launchApp } from './app'

// Covers what the unit tests cannot: a chord reaching its command, and the
// command reaching the screen. Model decisions live in shared/model.test.ts.

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
    // zsh publishes its directory as the window title, which the sidebar row
    // shows, so the row reports where the shell actually is.
    await app.runInPane('pty-1', 'cd /usr/local')
    await app.until('the first row follows the shell', (state) =>
      state.rows[0].label.endsWith('/usr/local')
    )

    await app.press(KEY.backslash, { cmd: true })
    const screen = await app.until('the new pane has published a title', (state) => {
      const bothRows = state.rows.length === 2
      if (!bothRows) {
        return false
      }
      return state.rows[1].label !== 'Terminal'
    })

    expect(screen.rows[1].label).toMatch(/\/usr\/local$/)
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
