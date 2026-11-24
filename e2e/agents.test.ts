import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { type AgentMenuReading, type App, launchApp, pollFor } from './app'

const MENU_POLL_TIMEOUT_MS = 8000
const SETTLE_PAUSE_MS = 500
const EXPECTED_HOOK_COUNT = 10

const STATUS_NOT_SET_UP = 'Not set up'
const STATUS_SET_UP = 'Set up'
const STATUS_NOT_INSTALLED = 'Not installed'
const ACTION_SET_UP = 'Set Up Claude Code'
const ACTION_REMOVE = 'Remove Setup'
const STATUS_OFF = 'Turned off in settings'
const OFFER_TEXT = 'Set up Claude Code to see which session needs you, restore conversations and fork them.'
const DONE_TEXT = 'Claude Code is set up. Markers start with your next session.'

const USER_COMMAND = '/some/user/script.sh'
const USER_SETTINGS = {
  hooks: {
    PreToolUse: [{ matcher: 'Bash', hooks: [{ type: 'command', command: USER_COMMAND }] }]
  }
}

interface HookCommandGroup {
  hooks: { command: string }[]
}

interface ClaudeSettings {
  hooks: Record<string, HookCommandGroup[]>
}

function scratchDirectory(name: string): string {
  return mkdtempSync(join(tmpdir(), `taco-shells-agents-${name}-e2e-`))
}

function scratchClaudeDirectory(): string {
  const claudeDir = scratchDirectory('claude')
  writeFileSync(join(claudeDir, 'settings.json'), JSON.stringify(USER_SETTINGS, null, 2))
  return claudeDir
}

function readSettingsText(claudeDir: string): string {
  return readFileSync(join(claudeDir, 'settings.json'), 'utf8')
}

function allCommands(claudeDir: string): string[] {
  const settings = JSON.parse(readSettingsText(claudeDir)) as ClaudeSettings
  const groups = Object.values(settings.hooks).flat()
  const hooks = groups.flatMap((group) => group.hooks)
  return hooks.map((hook) => hook.command)
}

function managedCommands(claudeDir: string, configDir: string): string[] {
  const prefix = `${join(configDir, 'hooks')}/`
  const commands = allCommands(claudeDir)
  return commands.filter((command) => command.startsWith(prefix))
}

function firstPreToolUseCommand(claudeDir: string): string {
  const settings = JSON.parse(readSettingsText(claudeDir)) as ClaudeSettings
  const firstGroup = settings.hooks.PreToolUse[0]
  return firstGroup.hooks[0].command
}

async function untilMenu(app: App, status: string, actionLabel: string): Promise<AgentMenuReading> {
  return pollFor(
    `the Agents menu to read ${status} with ${actionLabel}`,
    async () => {
      const reading = await app.agentMenu()
      if (reading.status !== status || reading.actionLabel !== actionLabel) {
        return null
      }
      return reading
    },
    MENU_POLL_TIMEOUT_MS
  )
}

async function untilBanners(app: App, description: string, expected: string[]): Promise<void> {
  await pollFor(
    `the strip to show ${description}`,
    async () => {
      const texts = await app.agentBanners()
      if (texts.length !== expected.length) {
        return null
      }
      if (texts.some((text, index) => text !== expected[index])) {
        return null
      }
      return texts
    },
    MENU_POLL_TIMEOUT_MS
  )
}

async function setUpFromMenu(app: App): Promise<void> {
  const before = await untilMenu(app, STATUS_NOT_SET_UP, ACTION_SET_UP)
  expect(before.enabled).toBe(true)
  await app.clickAgentAction()
  await untilMenu(app, STATUS_SET_UP, ACTION_REMOVE)
}

describe('the Agents menu', () => {
  it('sets up from the menu', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchClaudeDirectory()

    const app = await launchApp({ configDir, mainInspector: true, env: { CLAUDE_CONFIG_DIR: claudeDir } })
    try {
      await setUpFromMenu(app)

      const commands = managedCommands(claudeDir, configDir)
      expect(commands).toHaveLength(EXPECTED_HOOK_COUNT)
      for (const command of commands) {
        const mode = statSync(command).mode
        expect(mode & 0o111).not.toBe(0)
      }
      expect(firstPreToolUseCommand(claudeDir)).toBe(USER_COMMAND)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })

  it('removes the setup and does not put it back', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchClaudeDirectory()
    const env = { CLAUDE_CONFIG_DIR: claudeDir }

    const app = await launchApp({ configDir, mainInspector: true, env })
    let relaunched: App | null = null
    let firstClosed = false
    try {
      await setUpFromMenu(app)
      await untilBanners(app, 'the done line', [DONE_TEXT])
      await app.clickAgentAction()
      await untilMenu(app, STATUS_NOT_SET_UP, ACTION_SET_UP)
      await untilBanners(app, 'no strip', [])

      expect(managedCommands(claudeDir, configDir)).toHaveLength(0)
      expect(allCommands(claudeDir)).toContain(USER_COMMAND)
      const textBeforeRelaunch = readSettingsText(claudeDir)

      await app.close()
      firstClosed = true
      relaunched = await launchApp({ configDir, mainInspector: true, env })
      const reading = await untilMenu(relaunched, STATUS_NOT_SET_UP, ACTION_SET_UP)
      expect(reading.enabled).toBe(true)
      expect(readSettingsText(claudeDir)).toBe(textBeforeRelaunch)
    } finally {
      if (relaunched !== null) {
        await relaunched.close()
      }
      if (!firstClosed) {
        await app.close()
      }
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })

  it('keeps a development build away from settings it was not pointed at', async () => {
    const configDir = scratchDirectory('config')
    const home = scratchDirectory('home')
    mkdirSync(join(home, '.claude'))
    const settingsPath = join(home, '.claude', 'settings.json')
    writeFileSync(settingsPath, '{}')

    const app = await launchApp({
      configDir,
      mainInspector: true,
      env: { HOME: home, CLAUDE_CONFIG_DIR: '' }
    })
    try {
      const reading = await untilMenu(app, STATUS_NOT_SET_UP, ACTION_SET_UP)
      expect(reading.enabled).toBe(false)

      await app.clickAgentAction()
      await app.pause(SETTLE_PAUSE_MS)
      expect(readFileSync(settingsPath, 'utf8')).toBe('{}')
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(home, { recursive: true, force: true })
    }
  })

  it('notices an agent installed while the app was open', async () => {
    const configDir = scratchDirectory('config')
    const parent = scratchDirectory('parent')
    const claudeDir = join(parent, 'claude')

    const app = await launchApp({ configDir, mainInspector: true, env: { CLAUDE_CONFIG_DIR: claudeDir } })
    try {
      const before = await untilMenu(app, STATUS_NOT_INSTALLED, ACTION_SET_UP)
      expect(before.enabled).toBe(false)
      expect(await app.agentBanners()).toEqual([])

      mkdirSync(claudeDir)
      await app.emitInMain('did-become-active')
      const after = await untilMenu(app, STATUS_NOT_SET_UP, ACTION_SET_UP)
      expect(after.enabled).toBe(true)
      await untilBanners(app, 'the offer', [OFFER_TEXT])
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(parent, { recursive: true, force: true })
    }
  })

  it('offers once and sets up from the strip', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchClaudeDirectory()

    const app = await launchApp({ configDir, mainInspector: true, env: { CLAUDE_CONFIG_DIR: claudeDir } })
    try {
      await untilBanners(app, 'the offer', [OFFER_TEXT])
      await app.clickBannerButton('Set up')
      await untilBanners(app, 'the done line', [DONE_TEXT])
      expect(managedCommands(claudeDir, configDir)).toHaveLength(EXPECTED_HOOK_COUNT)

      await app.clickBannerButton('Dismiss')
      await untilBanners(app, 'no strip', [])
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })

  it('does not offer again after Not now', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchClaudeDirectory()
    const env = { CLAUDE_CONFIG_DIR: claudeDir }

    const app = await launchApp({ configDir, mainInspector: true, env })
    let relaunched: App | null = null
    let firstClosed = false
    try {
      await untilBanners(app, 'the offer', [OFFER_TEXT])
      await app.clickBannerButton('Not now')
      await untilBanners(app, 'no strip', [])

      await app.close()
      firstClosed = true
      relaunched = await launchApp({ configDir, mainInspector: true, env })
      const reading = await untilMenu(relaunched, STATUS_NOT_SET_UP, ACTION_SET_UP)
      expect(reading.enabled).toBe(true)
      await relaunched.pause(SETTLE_PAUSE_MS)
      expect(await relaunched.agentBanners()).toEqual([])
    } finally {
      if (relaunched !== null) {
        await relaunched.close()
      }
      if (!firstClosed) {
        await app.close()
      }
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })

  it('stays quiet while turned off, and offers once turned back on', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchClaudeDirectory()
    const appSettingsPath = join(configDir, 'settings.json')
    writeFileSync(appSettingsPath, JSON.stringify({ manageAgentHooks: false }))

    const app = await launchApp({ configDir, mainInspector: true, env: { CLAUDE_CONFIG_DIR: claudeDir } })
    try {
      const reading = await untilMenu(app, STATUS_OFF, ACTION_SET_UP)
      expect(reading.enabled).toBe(false)
      await app.pause(SETTLE_PAUSE_MS)
      expect(await app.agentBanners()).toEqual([])

      writeFileSync(appSettingsPath, JSON.stringify({ manageAgentHooks: true }))
      await untilBanners(app, 'the offer', [OFFER_TEXT])
      const turnedOn = await untilMenu(app, STATUS_NOT_SET_UP, ACTION_SET_UP)
      expect(turnedOn.enabled).toBe(true)
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })

  it('says so when Not now cannot be remembered', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchClaudeDirectory()
    const recordPath = join(configDir, 'agents.json')
    mkdirSync(recordPath)

    const app = await launchApp({ configDir, mainInspector: true, env: { CLAUDE_CONFIG_DIR: claudeDir } })
    try {
      await untilBanners(app, 'the offer', [OFFER_TEXT])
      await app.clickBannerButton('Not now')
      await pollFor(
        'the strip to say the decline was not recorded',
        async () => {
          const texts = await app.agentBanners()
          if (texts.length !== 1 || !texts[0].startsWith(`${recordPath} could not be written`)) {
            return null
          }
          return texts
        },
        MENU_POLL_TIMEOUT_MS
      )
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })

  it('says why when the settings file cannot be edited', async () => {
    const configDir = scratchDirectory('config')
    const claudeDir = scratchDirectory('claude')
    const settingsPath = join(claudeDir, 'settings.json')
    writeFileSync(settingsPath, '{ not json')

    const app = await launchApp({ configDir, mainInspector: true, env: { CLAUDE_CONFIG_DIR: claudeDir } })
    try {
      await untilBanners(app, 'the offer', [OFFER_TEXT])
      await app.clickBannerButton('Set up')
      const problem = `${settingsPath} could not be read as JSON, so nothing was changed.`
      await untilBanners(app, 'the problem', [problem])
      expect(readFileSync(settingsPath, 'utf8')).toBe('{ not json')
    } finally {
      await app.close()
      rmSync(configDir, { recursive: true, force: true })
      rmSync(claudeDir, { recursive: true, force: true })
    }
  })
})
