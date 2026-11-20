import { existsSync, mkdtempSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, KEY, launchApp } from './app'

// Covers what the unit tests cannot: a real drag reaching the model, and a
// moved terminal still working. The band rule lives in shared/drop.test.ts.

function reportedColumns(file: string): number {
  if (!existsSync(file)) {
    return 0
  }

  const reported = readFileSync(file, 'utf8')
  return Number(reported.trim())
}

let app: App

beforeEach(async () => {
  app = await launchApp()
})

afterEach(async () => {
  await app.close()
})

describe('dragging in the sidebar', () => {
  it('joins a terminal to another by a drop on its row, and the moved terminal still works', async () => {
    await app.press(KEY.t, { cmd: true })
    await app.until('a second terminal exists', (state) => state.groups === 2)

    const directory = mkdtempSync(join(tmpdir(), 'taco-shells-drag-'))
    const before = join(directory, 'before')
    const after = join(directory, 'after')

    await app.runInPane('pty-2', `tput cols > ${before}.part; mv ${before}.part ${before}`)
    await app.until('the shell has reported its width', () => reportedColumns(before) > 0)

    await app.dragRow(1, 0, 0.6)

    const screen = await app.until('both rows share one split', (state) => {
      return state.groups === 1 && state.rows.length === 2
    })
    expect(screen.rows.map((row) => row.connector)).toEqual(['is-first', 'is-last'])
    expect(screen.rows.map((row) => row.active)).toEqual([false, true])
    expect(screen.panes.map((pane) => pane.width)).toEqual([0.5, 0.5])
    expect(screen.terminalFocused).toBe(true)

    await app.runInPane('pty-2', `tput cols > ${after}.part; mv ${after}.part ${after}`)
    await app.until('the moved shell has reported its width', () => reportedColumns(after) > 0)

    const columnsBefore = reportedColumns(before)
    const columnsAfter = reportedColumns(after)
    expect(columnsAfter).toBeLessThan(columnsBefore * 0.6)
  })

  it('takes a pane out by the edge of a row, and moves a whole split by its bracket', async () => {
    await app.press(KEY.backslash, { cmd: true })
    await app.until('the terminal is split', (state) => state.rows.length === 2)

    await app.dragRow(1, 0, 0.1)

    const parted = await app.until('the pane stands alone', (state) => state.groups === 2)
    expect(parted.rows.map((row) => row.connector)).toEqual(['', ''])
    expect(parted.rows.map((row) => row.active)).toEqual([true, false])

    await app.dragRow(0, 1, 0.6)
    await app.until('they are a split again', (state) => state.groups === 1)
    await app.press(KEY.t, { cmd: true })
    await app.until('a terminal sits below the split', (state) => state.groups === 2)

    await app.dragBracket(0, 2, 0.9)

    const moved = await app.until('the split sits below the terminal', (state) => {
      return state.rows[0].connector === ''
    })
    expect(moved.rows.map((row) => row.connector)).toEqual(['', 'is-first', 'is-last'])
    expect(moved.groups).toBe(2)
  })
})
