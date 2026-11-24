import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, launchApp } from './app'

const NAVIGATION_PAUSE_MS = 500

let app: App

beforeEach(async () => {
  app = await launchApp()
})

afterEach(async () => {
  await app.close()
})

describe('the window', () => {
  it('stays on the app when the page is sent to a file, as a dropped file would send it', async () => {
    await app.cdp('Runtime.evaluate', { expression: "window.location.href = 'file:///etc/hosts'" })
    await app.pause(NAVIGATION_PAUSE_MS)

    const state = await app.screen()
    expect(state.rows).toHaveLength(1)
  })
})
