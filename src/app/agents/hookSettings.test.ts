import { describe, expect, it } from 'vitest'
import type { HarnessSetup, HookEntry } from '@agents/types'
import { addEntries, hookWiringOf, removeHandEntries, removeManagedEntries } from './hookSettings'

const HOOKS_DIRECTORY = '/Users/x/.taco-shells/hooks'
const CLONE_DIRECTORY = '/Users/x/code/taco-shells/hooks'
const LABEL = '/Users/x/.claude/settings.json'
const ATTENTION = 'taco-shells-attention.sh'
const SESSION = 'taco-shells-claude-session.sh'

const ENTRIES: readonly HookEntry[] = [
  { event: 'Notification', script: ATTENTION, async: false },
  { event: 'Stop', script: ATTENTION, async: false },
  { event: 'SessionStart', script: SESSION, async: false },
  { event: 'SessionEnd', script: SESSION, async: false },
  { event: 'UserPromptSubmit', script: SESSION, async: true },
  { event: 'PreToolUse', script: SESSION, async: true },
  { event: 'PostToolUse', script: SESSION, async: true },
  { event: 'Notification', script: SESSION, async: true },
  { event: 'Stop', script: SESSION, async: true },
  { event: 'StopFailure', script: SESSION, async: true }
]

const SETUP: HarnessSetup = {
  productName: 'Claude Code',
  commands: ['claude'],
  configDirectoryVariable: 'CLAUDE_CONFIG_DIR',
  defaultConfigDirectory: '.claude',
  settingsFile: 'settings.json',
  entries: ENTRIES
}

const USER_GROUP = { matcher: 'Bash', hooks: [{ type: 'command', command: '/some/user/script.sh' }] }

interface HookObject {
  type: string
  command: string
  async?: boolean
}

interface Group {
  matcher?: string
  hooks: HookObject[]
}

function parsedHooks(text: string | null): Record<string, Group[]> {
  expect(text).not.toBeNull()
  const parsed = JSON.parse(text as string)
  return parsed.hooks
}

function managedText(): string {
  const result = addEntries(null, ENTRIES, HOOKS_DIRECTORY, LABEL)
  return result.text as string
}

function cloneGroup(script: string): Group {
  return { hooks: [{ type: 'command', command: `${CLONE_DIRECTORY}/${script}` }] }
}

describe('addEntries', () => {
  it('adds every entry to an empty file', () => {
    const result = addEntries(null, ENTRIES, HOOKS_DIRECTORY, LABEL)
    const hooks = parsedHooks(result.text)
    for (const entry of ENTRIES) {
      const groups = hooks[entry.event]
      const written = groups.flatMap((group) => group.hooks)
      const hook = written.find((candidate) => candidate.command === `${HOOKS_DIRECTORY}/${entry.script}`)
      let expected: HookObject = { type: 'command', command: `${HOOKS_DIRECTORY}/${entry.script}` }
      if (entry.async) {
        expected = { ...expected, async: true }
      }
      expect(hook).toEqual(expected)
    }
    expect(result.text?.endsWith('\n')).toBe(true)
  })

  it("keeps the user's own hooks and formatting", () => {
    const original = [
      '{',
      '    "model": "opus",',
      '    "permissions": {',
      '        "allow": ["Bash(ls:*)"]',
      '    },',
      '    "hooks": {',
      '        "PreToolUse": [',
      '            {',
      '                "matcher": "Bash",',
      '                "hooks": [{ "type": "command", "command": "/some/user/script.sh" }]',
      '            }',
      '        ]',
      '    }',
      '}',
      ''
    ].join('\n')
    const result = addEntries(original, ENTRIES, HOOKS_DIRECTORY, LABEL)
    const hooks = parsedHooks(result.text)
    expect(hooks.PreToolUse[0]).toEqual(USER_GROUP)
    expect(hooks.PreToolUse[1].hooks[0].command).toBe(`${HOOKS_DIRECTORY}/${SESSION}`)
    const output = result.text as string
    const untouchedBlocks = [
      '    "model": "opus",\n    "permissions": {\n        "allow": ["Bash(ls:*)"]\n    },\n',
      '            {\n                "matcher": "Bash",\n                "hooks": [{ "type": "command", "command": "/some/user/script.sh" }]\n            }'
    ]
    for (const block of untouchedBlocks) {
      expect(output).toContain(block)
    }

    const managedGroupLine = '\n            {\n                "hooks": [\n'
    expect(output).toContain(managedGroupLine)

    const removed = removeManagedEntries(output, HOOKS_DIRECTORY, LABEL)
    expect(removed.text).toBe(original)
  })

  it('indents with tabs in a file that does', () => {
    const original = '{\n\t"model": "opus"\n}\n'
    const result = addEntries(original, ENTRIES.slice(0, 1), HOOKS_DIRECTORY, LABEL)
    const output = result.text as string
    expect(output).toContain('{\n\t"model": "opus",\n\t"hooks": {\n\t\t"Notification": [\n')

    const removed = removeManagedEntries(output, HOOKS_DIRECTORY, LABEL)
    expect(removed.text).toBe(original)
  })

  it('adds nothing that is already there', () => {
    const first = managedText()
    const second = addEntries(first, ENTRIES, HOOKS_DIRECTORY, LABEL)
    expect(second).toEqual({ text: null, problem: null })

    const parsed = JSON.parse(first)
    delete parsed.hooks.StopFailure
    const withoutOne = JSON.stringify(parsed, null, 2)
    const third = addEntries(withoutOne, ENTRIES, HOOKS_DIRECTORY, LABEL)
    expect(parsedHooks(third.text)).toEqual(parsedHooks(first))
  })

  it('treats an empty file as an empty object', () => {
    const fromNothing = addEntries(null, ENTRIES, HOOKS_DIRECTORY, LABEL)
    for (const empty of ['', '  \n']) {
      const result = addEntries(empty, ENTRIES, HOOKS_DIRECTORY, LABEL)
      expect(result.text).toBe(fromNothing.text)
    }
  })

  it('quotes a path with a space', () => {
    const spaced = addEntries(null, ENTRIES.slice(0, 1), '/Users/a b/.taco-shells/hooks', LABEL)
    const plain = addEntries(null, ENTRIES.slice(0, 1), HOOKS_DIRECTORY, LABEL)
    const spacedCommand = parsedHooks(spaced.text).Notification[0].hooks[0].command
    const plainCommand = parsedHooks(plain.text).Notification[0].hooks[0].command
    expect(spacedCommand).toBe(`"/Users/a b/.taco-shells/hooks/${ATTENTION}"`)
    expect(plainCommand).toBe(`${HOOKS_DIRECTORY}/${ATTENTION}`)
  })
})

describe('hookWiringOf', () => {
  it('reads the wiring', () => {
    const userOnly = JSON.stringify({ hooks: { PreToolUse: [USER_GROUP] } })
    const clone = JSON.stringify({ hooks: { SessionStart: [cloneGroup(SESSION)] } })
    const managed = managedText()
    const both = addEntries(clone, ENTRIES, HOOKS_DIRECTORY, LABEL).text
    expect(hookWiringOf(null, SETUP, HOOKS_DIRECTORY)).toBe('none')
    expect(hookWiringOf('{ not json', SETUP, HOOKS_DIRECTORY)).toBe('none')
    expect(hookWiringOf(userOnly, SETUP, HOOKS_DIRECTORY)).toBe('none')

    const lookalikes = [
      '/opt/Users/x/.taco-shells/hooks/other.sh',
      `${HOOKS_DIRECTORY}-extra/other.sh`,
      `/x/my-${ATTENTION}.bak`
    ]
    for (const command of lookalikes) {
      const text = JSON.stringify({ hooks: { Stop: [{ hooks: [{ type: 'command', command }] }] } })
      expect(hookWiringOf(text, SETUP, HOOKS_DIRECTORY)).toBe('none')
    }

    const spacedClone = `"/Users/a b/code/taco-shells/hooks/${SESSION}"`
    const spacedGroup = { hooks: [{ type: 'command', command: spacedClone }] }
    const spacedText = JSON.stringify({ hooks: { Stop: [spacedGroup] } })
    expect(hookWiringOf(spacedText, SETUP, HOOKS_DIRECTORY)).toBe('byHand')

    const spacedDirectory = '/Users/a b/.taco-shells/hooks'
    const spacedManaged = addEntries(null, ENTRIES, spacedDirectory, LABEL).text
    expect(hookWiringOf(spacedManaged, SETUP, spacedDirectory)).toBe('managed')
    expect(hookWiringOf(managed, SETUP, HOOKS_DIRECTORY)).toBe('managed')
    expect(hookWiringOf(clone, SETUP, HOOKS_DIRECTORY)).toBe('byHand')
    expect(hookWiringOf(both, SETUP, HOOKS_DIRECTORY)).toBe('byHand')
  })
})

describe('removal', () => {
  it('removes only what the app wrote', () => {
    const promptGroup = { hooks: [{ type: 'prompt', prompt: 'Is this safe?' }] }
    const withUser = JSON.stringify({ hooks: { PreToolUse: [USER_GROUP, promptGroup] } }, null, 2)
    const mixed = addEntries(withUser, ENTRIES, HOOKS_DIRECTORY, LABEL).text as string
    const removed = removeManagedEntries(mixed, HOOKS_DIRECTORY, LABEL)
    const hooks = parsedHooks(removed.text)
    expect(Object.keys(hooks)).toEqual(['PreToolUse'])
    expect(hooks.PreToolUse).toEqual([USER_GROUP, promptGroup])

    const withKey = addEntries('{ "model": "opus" }', ENTRIES, HOOKS_DIRECTORY, LABEL).text as string
    const cleaned = removeManagedEntries(withKey, HOOKS_DIRECTORY, LABEL)
    expect(JSON.parse(cleaned.text as string)).toEqual({ model: 'opus' })
  })

  it('removes hand-made entries for a switch', () => {
    const unrelated = { hooks: [{ type: 'command', command: '/some/user/script.sh' }] }
    const start = JSON.stringify({
      hooks: {
        Stop: [cloneGroup(ATTENTION), unrelated],
        SessionStart: [cloneGroup(SESSION)],
        Notification: [{ hooks: [...cloneGroup(ATTENTION).hooks, ...unrelated.hooks] }]
      }
    })
    const mixed = addEntries(start, ENTRIES, HOOKS_DIRECTORY, LABEL).text as string
    const removed = removeHandEntries(mixed, SETUP, HOOKS_DIRECTORY, LABEL)
    const hooks = parsedHooks(removed.text)
    const commands = Object.values(hooks).flat().flatMap((group) => group.hooks.map((hook) => hook.command))
    expect(commands.some((command) => command.startsWith(CLONE_DIRECTORY))).toBe(false)
    expect(commands.filter((command) => command.startsWith(HOOKS_DIRECTORY))).toHaveLength(10)
    expect(commands.filter((command) => command === '/some/user/script.sh')).toHaveLength(2)
    expect(hooks.Notification[0].hooks).toEqual(unrelated.hooks)
  })
})

describe('refusals', () => {
  it('refuses a file it cannot read', () => {
    const problem = `${LABEL} could not be read as JSON, so nothing was changed.`
    const broken = '{ not json'
    expect(addEntries(broken, ENTRIES, HOOKS_DIRECTORY, LABEL)).toEqual({ text: null, problem })
    expect(removeManagedEntries(broken, HOOKS_DIRECTORY, LABEL)).toEqual({ text: null, problem })
    expect(removeHandEntries(broken, SETUP, HOOKS_DIRECTORY, LABEL)).toEqual({ text: null, problem })
  })

  it('refuses hooks that is not an object', () => {
    const notObject = addEntries('{"hooks": []}', ENTRIES, HOOKS_DIRECTORY, LABEL)
    expect(notObject).toEqual({
      text: null,
      problem: `${LABEL}: "hooks" must be an object, so nothing was changed.`
    })
    const notList = addEntries('{"hooks": {"Stop": "nope"}}', ENTRIES, HOOKS_DIRECTORY, LABEL)
    expect(notList).toEqual({
      text: null,
      problem: `${LABEL}: "hooks.Stop" must be a list, so nothing was changed.`
    })
  })
})
