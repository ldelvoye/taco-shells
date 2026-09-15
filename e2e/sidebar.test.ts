import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, launchApp } from './app'

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
