import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { KEY, launchApp } from './app'

const SAVED_WORKSPACE = {
  version: 1,
  groups: [
    { panes: [{ width: 1, cwd: '/usr/local', claude: null }], activePane: 0 },
    {
      panes: [
        { width: 0.3, cwd: '/private/tmp', claude: null },
        { width: 0.7, cwd: '/nonexistent-dir-for-restore-test', claude: null }
      ],
      activePane: 1
    }
  ],
  activeGroup: 1
}

function scratchConfigDirectory(): string {
  return mkdtempSync(join(tmpdir(), 'taco-shells-restore-e2e-'))
}

function savedSnapshot(configDir: string): { groups: Array<{ panes: Array<{ cwd: string }> }> } {
  const text = readFileSync(join(configDir, 'workspace.json'), 'utf8')
  return JSON.parse(text)
}

describe('restoring a saved workspace', () => {
  it('reopens the saved groups with their shells in the saved directories', async () => {
    const configDir = scratchConfigDirectory()
    writeFileSync(join(configDir, 'workspace.json'), JSON.stringify(SAVED_WORKSPACE))

    const app = await launchApp({ configDir, expectedRows: 3 })
    try {
      const screen = await app.screen()
      expect(screen.rows).toHaveLength(3)
      expect(screen.panes).toHaveLength(2)
      expect(screen.panes[0].width).toBeCloseTo(0.3, 2)
      expect(screen.panes[1].width).toBeCloseTo(0.7, 2)
      expect(screen.dividers).toBe(1)
      expect(screen.rows[2].active).toBe(true)

      await app.quitGracefully()

      const snapshot = savedSnapshot(configDir)
      expect(snapshot.groups[0].panes[0].cwd).toBe('/usr/local')
      expect(snapshot.groups[1].panes[0].cwd).toBe('/private/tmp')
      expect(snapshot.groups[1].panes[1].cwd).toBe(homedir())
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })

  it('a corrupt snapshot opens one fresh terminal', async () => {
    const configDir = scratchConfigDirectory()
    writeFileSync(join(configDir, 'workspace.json'), '{"version":1,"groups":"no"}')

    const app = await launchApp({ configDir })
    try {
      const screen = await app.screen()
      expect(screen.rows).toHaveLength(1)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })

  it('quitting with no terminals writes an empty snapshot', async () => {
    const configDir = scratchConfigDirectory()

    const app = await launchApp({ configDir })
    try {
      await app.press(KEY.w, { cmd: true })
      await app.waitForExit()

      const snapshot = savedSnapshot(configDir)
      expect(snapshot.groups).toEqual([])
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })
})
