import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { type App, KEY, launchApp } from './app'

// Covers what the unit tests cannot: the popups and a real drag reaching the
// model, focus after a collapse, and groups surviving a restart.

const KEYS_SETTLE_MS = 400

async function startGroupFromRow(app: App, rowIndex: number): Promise<void> {
  await app.rightClickRow(rowIndex)
  await app.clickNewGroupItem()
  await app.until('the group has its name field open', (state) => {
    return state.headers.length === 1 && state.nameFieldFocused
  })
}

describe('tab groups', () => {
  it('starts a group from a row, names it, and keeps typing out of the terminals', async () => {
    const app = await launchApp()
    try {
      await app.press(KEY.t, { cmd: true })
      await app.until('a second terminal exists', (state) => state.rows.length === 2)

      await startGroupFromRow(app, 0)
      await app.typeText('pebbles')
      await app.press(KEY.w, { cmd: true })
      await app.pause(KEYS_SETTLE_MS)

      const typed = await app.screen()
      expect(typed.rows).toHaveLength(2)

      await app.press(KEY.enter)
      const screen = await app.until('the popup is closed and the terminal has the keyboard', (state) => {
        return state.terminalFocused && !state.nameFieldFocused
      })
      expect(screen.headers.map((header) => header.name)).toEqual(['pebbles'])
      expect(screen.rows.map((row) => row.grouped)).toEqual([true, false])
    } finally {
      await app.close()
    }
  })

  it('moves focus out of a group that collapses, and a new terminal joins the group it was opened from', async () => {
    const app = await launchApp()
    try {
      await app.press(KEY.t, { cmd: true })
      await app.until('a second terminal exists', (state) => state.rows.length === 2)

      await startGroupFromRow(app, 1)
      await app.press(KEY.escape)
      await app.until('the popup is closed', (state) => !state.nameFieldFocused)

      await app.press(KEY.t, { cmd: true })
      const opened = await app.until('a third terminal joined the group', (state) => {
        return state.rows.length === 3
      })
      expect(opened.rows.map((row) => row.grouped)).toEqual([false, true, true])
      expect(opened.rows[2].active).toBe(true)

      await app.clickHeader(0)
      const collapsed = await app.until('the group is collapsed', (state) => {
        return state.headers.length === 1 && state.headers[0].collapsed
      })
      expect(collapsed.headers[0].count).toBe(2)
      expect(collapsed.rows).toHaveLength(1)
      expect(collapsed.rows[0].active).toBe(true)
      expect(collapsed.terminalFocused).toBe(true)
    } finally {
      await app.close()
    }
  })

  it('drags a terminal into a group and brings the groups back after a restart', async () => {
    const configDir = mkdtempSync(join(tmpdir(), 'taco-shells-tab-groups-e2e-'))
    let app = await launchApp({ configDir })
    try {
      await app.press(KEY.t, { cmd: true })
      await app.until('a second terminal exists', (state) => state.rows.length === 2)

      await startGroupFromRow(app, 0)
      await app.typeText('kept')
      await app.press(KEY.enter)
      await app.until('the group is named', (state) => state.headers[0].name === 'kept')

      await app.dragRowToHeader(1, 0, 0.6)
      const dragged = await app.until('both rows are in the group', (state) => {
        return state.rows.length === 2 && state.rows.every((row) => row.grouped)
      })
      expect(dragged.headers).toHaveLength(1)

      await app.quitGracefully()

      app = await launchApp({ configDir, expectedRows: 2 })
      const restored = await app.screen()
      expect(restored.headers.map((header) => header.name)).toEqual(['kept'])
      expect(restored.rows.map((row) => row.grouped)).toEqual([true, true])
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })
})
