import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, KEY, launchApp } from './app'

let app: App

// Several titles rather than one: the rounding shortfall depends on the width of
// the particular text, so a single string can pass while the bug is back.
const LONG_TITLES = [
  'The quick brown fox jumps over the lazy dog',
  'oiehnweoignewoignweoignweogwoeingwoein',
  '~/code/taco-shells/src/renderer/sidebar'
]

beforeEach(async () => {
  app = await launchApp()
})

afterEach(async () => {
  await app.close()
})

describe('the sidebar divider', () => {
  it('widens the sidebar by the dragged amount and stops at half the window width', async () => {
    await app.dragSidebarDivider(-2000)
    const collapsed = await app.until(
      'the sidebar settles at its floor',
      (state) => state.sidebarWidth <= 145
    )

    await app.dragSidebarDivider(80)
    const widened = await app.until(
      'the sidebar has widened',
      (state) => state.sidebarWidth > collapsed.sidebarWidth + 70
    )
    expect(widened.sidebarWidth - collapsed.sidebarWidth).toBeGreaterThanOrEqual(76)
    expect(widened.sidebarWidth - collapsed.sidebarWidth).toBeLessThanOrEqual(84)

    await app.dragSidebarDivider(5000)
    const maxed = await app.until(
      'the sidebar has stopped growing',
      (state) => state.sidebarWidth > widened.sidebarWidth
    )

    const ceiling = maxed.windowWidth / 2
    expect(maxed.sidebarWidth).toBeGreaterThanOrEqual(ceiling - 5)
    expect(maxed.sidebarWidth).toBeLessThanOrEqual(ceiling + 5)
  })

  it.each(LONG_TITLES)('fits the sidebar to the row label "%s"', async (title) => {
    await app.dragSidebarDivider(-2000)
    await app.until('the sidebar settles at its floor', (state) => state.sidebarWidth <= 145)

    await app.runInPane('pty-1', `printf '\\033]0;${title}\\007'; sleep 60`)
    await app.until(
      'the row label shows the long title, truncated',
      (state) => state.rows[0].label === title && state.labelsTruncated
    )

    await app.doubleClickSidebarDivider()
    await app.until('the label is no longer truncated', (state) => !state.labelsTruncated)
  })
})

describe('row colors', () => {
  it('colors the active row from the keyboard, and the same chord clears it', async () => {
    const chord = { cmd: true, alt: true }

    await app.press(KEY.digit5, chord)
    await app.until('the row is blue', (state) => state.rows[0].color === 'blue')

    await app.press(KEY.digit5, chord)
    await app.until('the row has no color', (state) => state.rows[0].color === null)
  })

  it('previews a hovered dot on the row and keeps a color only when clicked', async () => {
    await app.rightClickRow(0)
    await app.until('the picker is open', (state) => state.colorPickerOpen)

    await app.hoverColorDot('green')
    await app.until('the row previews green', (state) => state.rows[0].color === 'green')

    await app.press(KEY.escape)
    const dismissed = await app.until('the picker closes', (state) => !state.colorPickerOpen)
    expect(dismissed.rows[0].color).toBeNull()
    expect(dismissed.terminalFocused).toBe(true)

    await app.rightClickRow(0)
    await app.until('the picker is open again', (state) => state.colorPickerOpen)
    await app.clickColorDot('red')
    const picked = await app.until('the picker closes', (state) => !state.colorPickerOpen)
    expect(picked.rows[0].color).toBe('red')
    expect(picked.terminalFocused).toBe(true)

    await app.rightClickRow(0)
    await app.until('the picker is open again', (state) => state.colorPickerOpen)
    await app.hoverColorDot('none')
    await app.until('the row previews no color', (state) => state.rows[0].color === null)
    await app.hoverRow(0)
    await app.until('the saved color is back', (state) => state.rows[0].color === 'red')
    await app.clickColorDot('none')
    const removed = await app.until('the picker closes', (state) => !state.colorPickerOpen)
    expect(removed.rows[0].color).toBeNull()
  })

  it('previews on the right-clicked row alone, and a click elsewhere drops it', async () => {
    await app.press(KEY.t, { cmd: true })
    await app.until('a second row appears', (state) => state.rows.length === 2)

    await app.rightClickRow(0)
    await app.until('the picker is open', (state) => state.colorPickerOpen)
    await app.hoverColorDot('green')
    const previewing = await app.until(
      'the first row previews green',
      (state) => state.rows[0].color === 'green'
    )
    expect(previewing.rows[1].color).toBeNull()
    expect(previewing.rows[1].active).toBe(true)

    await app.clickVisiblePane()
    const dismissed = await app.until('the picker closes', (state) => !state.colorPickerOpen)
    expect(dismissed.rows[0].color).toBeNull()
  })

  it('keeps the Esc that closes the picker away from the terminal', async () => {
    const script =
      "printf '\\033]0;ready\\007'; stty raw -echo; dd bs=1 count=1 >/dev/null 2>&1; " +
      "printf '\\033]0;got-a-key\\007'; sleep 60"
    await app.runInPane('pty-1', script)
    await app.until('the terminal is reading a key', (state) => state.rows[0].label === 'ready')

    await app.rightClickRow(0)
    await app.until('the picker is open', (state) => state.colorPickerOpen)
    await app.press(KEY.escape)
    await app.until('the picker closes', (state) => !state.colorPickerOpen)
    await app.pause(300)
    const afterPicker = await app.screen()
    expect(afterPicker.rows[0].label).toBe('ready')

    await app.press(KEY.escape)
    await app.until('the next Esc reaches it', (state) => state.rows[0].label === 'got-a-key')
  })
})
