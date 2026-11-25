import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { type App, launchApp } from './app'
import { KEY } from './harness/keys'
import {
  boxOf,
  centreOf,
  clickAt,
  clickButtonWithin,
  hover,
  ROW_HANDLE_OFFSET,
  rowBox
} from './harness/mouse'
import { pollFor } from './harness/poll'

// Covers what the unit tests cannot: the popups and a real drag reaching the
// model, focus after a collapse, and groups surviving a restart.

const KEYS_SETTLE_MS = 400
const SHELL_EXIT_TIMEOUT_MS = 5000

async function clickNewGroupItem(app: App): Promise<void> {
  const centre = await hover(app.page, '.row-new-group-item', 0)
  await clickAt(app.page, centre, 'left')
}

async function clickHeader(app: App, headerIndex: number): Promise<void> {
  const centre = await centreOf(app.page, '.tab-group-header', headerIndex)
  await clickAt(app.page, centre, 'left')
}

async function clickRow(app: App, rowIndex: number): Promise<void> {
  const centre = await centreOf(app.page, '.terminal-row', rowIndex)
  await clickAt(app.page, centre, 'left')
}

async function clickNewTerminalButton(app: App): Promise<void> {
  const centre = await centreOf(app.page, '.sidebar-new', 0)
  await clickAt(app.page, centre, 'left')
}

async function clickNewInHeader(app: App, headerIndex: number): Promise<void> {
  await clickButtonWithin(app.page, '.tab-group-header', headerIndex, '.tab-group-new')
}

async function clickCloseOnRow(app: App, rowIndex: number): Promise<void> {
  await clickButtonWithin(app.page, '.terminal-row', rowIndex, '.row-close')
}

async function typeText(app: App, text: string): Promise<void> {
  await app.page.send('Input.insertText', { text })
}

async function dragRowToHeader(
  app: App,
  fromRow: number,
  headerIndex: number,
  fraction: number
): Promise<void> {
  const from = await rowBox(app.page, fromRow)
  const to = await boxOf(app.page, '.tab-group-header', headerIndex)
  const source = { x: from.left + ROW_HANDLE_OFFSET, y: from.top + from.height / 2 }
  const target = { x: to.left + ROW_HANDLE_OFFSET, y: to.top + fraction * to.height }
  await app.drags.drag(source, target)
}

async function startGroupFromRow(app: App, rowIndex: number): Promise<void> {
  await app.rightClickRow(rowIndex)
  await clickNewGroupItem(app)
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
      await typeText(app, 'pebbles')
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

      await clickHeader(app, 0)
      const collapsed = await app.until('the group is collapsed', (state) => {
        return state.headers.length === 1 && state.headers[0].collapsed
      })
      expect(collapsed.headers[0].count).toBe(2)
      expect(collapsed.rows).toHaveLength(1)
      expect(collapsed.rows[0].active).toBe(true)
      expect(collapsed.terminalFocused).toBe(true)

      await clickHeader(app, 0)
      await app.until('the group is open again and the terminal has the keyboard back', (state) => {
        return !state.headers[0].collapsed && state.terminalFocused
      })
    } finally {
      await app.close()
    }
  })

  it('opens and closes terminals from the sidebar buttons', async () => {
    const app = await launchApp()
    try {
      await clickNewTerminalButton(app)
      const second = await app.until('a second terminal is active and focused', (state) => {
        return state.rows.length === 2 && state.rows[1].active && state.terminalFocused
      })
      expect(second.rows).toHaveLength(2)

      await startGroupFromRow(app, 0)
      await app.press(KEY.escape)
      await app.until('the popup is closed', (state) => !state.nameFieldFocused)
      await clickRow(app, 0)
      await app.until('the grouped terminal is active', (state) => state.rows[0].active)

      await clickNewTerminalButton(app)
      const ungrouped = await app.until('a third terminal sits at the bottom', (state) => {
        return state.rows.length === 3
      })
      expect(ungrouped.rows.map((row) => row.grouped)).toEqual([true, false, false])

      await clickHeader(app, 0)
      await app.until('the group is collapsed', (state) => state.headers[0].collapsed)

      await clickNewInHeader(app, 0)
      const joined = await app.until('the group is open with a new active terminal', (state) => {
        return state.rows.length === 4 && !state.headers[0].collapsed && state.terminalFocused
      })
      expect(joined.rows.map((row) => row.grouped)).toEqual([true, true, false, false])
      expect(joined.rows[1].active).toBe(true)

      const shellsBefore = app.shellCount()
      await clickCloseOnRow(app, 2)
      const closed = await app.until('the closed terminal is gone', (state) => state.rows.length === 3)
      expect(closed.rows[1].active).toBe(true)
      expect(closed.terminalFocused).toBe(true)
      await pollFor(
        'the closed terminal shell to exit',
        async () => {
          if (app.shellCount() === shellsBefore - 1) {
            return true
          }
          return null
        },
        SHELL_EXIT_TIMEOUT_MS
      )
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
      await typeText(app, 'kept')
      await app.press(KEY.enter)
      await app.until('the group is named', (state) => state.headers[0].name === 'kept')

      await dragRowToHeader(app, 1, 0, 0.6)
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
