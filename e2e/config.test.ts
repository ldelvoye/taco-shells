import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { KEY, type App, type Key, type Screen, launchApp, pollFor } from './app'

const KEY_D: Key = { code: 'KeyD', text: 'd', virtualKeyCode: 68 }

const REBIND_POLL_TIMEOUT_MS = 8000

const LIGHT_CHROME_BACKGROUND = '#f3f3f3'
const DARK_CHROME_BACKGROUND = '#252526'

function scratchConfigDirectory(): string {
  return mkdtempSync(join(tmpdir(), 'taqueria-config-e2e-'))
}

async function pressUntilSplit(app: App): Promise<Screen> {
  return pollFor(
    'cmd+shift+d to split the pane once the keybinding was written',
    async () => {
      await app.press(KEY_D, { cmd: true, shift: true })
      const screen = await app.screen()
      if (screen.rows.length !== 2) {
        return null
      }
      return screen
    },
    REBIND_POLL_TIMEOUT_MS
  )
}

async function untilChromeBackground(app: App, expected: string): Promise<void> {
  await pollFor(
    `the chrome background to become ${expected}`,
    async () => {
      const background = await app.chromeBackground()
      if (background !== expected) {
        return null
      }
      return background
    },
    REBIND_POLL_TIMEOUT_MS
  )
}

describe('keybindings.json', () => {
  it('a rebinding written before launch takes effect', async () => {
    const configDir = scratchConfigDirectory()
    writeFileSync(join(configDir, 'keybindings.json'), '{ "cmd+shift+d": "terminal.split" }')

    const app = await launchApp({ configDir })
    try {
      await app.press(KEY_D, { cmd: true, shift: true })
      const screen = await app.until('the group holds two panes', (state) => state.rows.length === 2)
      expect(screen.rows).toHaveLength(2)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })

  it('a rebinding written while running takes effect with no relaunch', async () => {
    const configDir = scratchConfigDirectory()

    const app = await launchApp({ configDir })
    try {
      await app.press(KEY_D, { cmd: true, shift: true })
      await app.pause(500)
      const unchanged = await app.screen()
      expect(unchanged.rows).toHaveLength(1)

      writeFileSync(join(configDir, 'keybindings.json'), '{ "cmd+shift+d": "terminal.split" }')

      const screen = await pressUntilSplit(app)
      expect(screen.rows).toHaveLength(2)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })
})

describe('cmd+,', () => {
  it('opens the settings file in a new terminal, running the shell EDITOR', async () => {
    const configDir = scratchConfigDirectory()
    const openedPath = join(configDir, 'opened-by-editor.txt')
    const fakeEditor = join(configDir, 'editor.sh')
    writeFileSync(fakeEditor, `#!/bin/sh\nprintf '%s' "$1" > '${openedPath}'\nsleep 30\n`, {
      mode: 0o755
    })

    const app = await launchApp({ configDir, env: { EDITOR: fakeEditor } })
    try {
      await app.press(KEY.comma, { cmd: true })
      await app.until('a second terminal opens', (state) => state.rows.length === 2)

      const opened = await pollFor(
        'the editor to be handed a path',
        async () => {
          try {
            return readFileSync(openedPath, 'utf8')
          } catch {
            return null
          }
        },
        REBIND_POLL_TIMEOUT_MS
      )

      expect(opened).toBe(join(configDir, 'settings.json'))
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })
})

describe('settings.json', () => {
  it('a broken file still leaves the app running with a terminal, naming the file as a problem', async () => {
    const configDir = scratchConfigDirectory()
    writeFileSync(join(configDir, 'settings.json'), '{ "fontSize": }')

    const app = await launchApp({ configDir })
    try {
      const screen = await app.screen()
      expect(screen.rows).toHaveLength(1)

      const problems = await app.configProblems()
      const namesSettingsFile = problems.some((problem) => problem.includes('settings.json'))
      expect(namesSettingsFile).toBe(true)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })

  it('a pinned theme follows the file, and a bad value beside it costs only itself', async () => {
    const configDir = scratchConfigDirectory()
    const settingsPath = join(configDir, 'settings.json')
    writeFileSync(settingsPath, '{ "theme": "light", "fontSize": "large" }')

    const app = await launchApp({ configDir })
    try {
      const background = await app.chromeBackground()
      expect(background).toBe(LIGHT_CHROME_BACKGROUND)

      const problems = await app.configProblems()
      const namesFontSize = problems.some((problem) => problem.includes('fontSize'))
      expect(namesFontSize).toBe(true)

      writeFileSync(settingsPath, '{ "theme": "dark", "fontSize": "large" }')
      await untilChromeBackground(app, DARK_CHROME_BACKGROUND)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
    }
  })
})
