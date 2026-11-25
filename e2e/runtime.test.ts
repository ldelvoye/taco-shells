import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { launchApp } from './app'
import { evaluate } from './harness/connection'
import { KEY } from './harness/keys'

const SAVE_SETTLE_MS = 600
const PROBLEM = 'The terminals stopped because their background process exited'

// The shells live in a process of their own. Only the real app shows what the
// window does when that process dies, and what the next launch finds saved.

describe('the terminal process', () => {
  it('when it is killed the terminals go and the window says why, and the next launch brings them back', async () => {
    const configDir = mkdtempSync(join(tmpdir(), 'taco-shells-runtime-e2e-'))
    const app = await launchApp({ configDir })
    try {
      await app.press(KEY.t, { cmd: true })
      await app.until('the second terminal is open', (screen) => screen.rows.length === 2)
      // The save is written a moment after the arrangement changes.
      await app.pause(SAVE_SETTLE_MS)

      process.kill(app.terminalProcessPid(), 'SIGKILL')

      await app.until('the terminals are gone', (screen) => screen.rows.length === 0)
      const banners = await evaluate(app.page, `document.querySelector('.banner-lines').textContent`)
      expect(banners).toContain(PROBLEM)

      await app.pause(SAVE_SETTLE_MS)
      const stillOpen = await app.screen()
      expect(stillOpen.rows).toHaveLength(0)

      await app.quitGracefully()

      const reopened = await launchApp({ configDir, expectedRows: 2 })
      try {
        expect(reopened.shellCount()).toBe(2)
      } finally {
        await reopened.close()
      }
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })
})
