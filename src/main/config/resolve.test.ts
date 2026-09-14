import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { isCommandId } from '@shared/commands'
import { DEFAULT_SETTINGS } from '@shared/settings'
import { parseJsonc } from './jsonc'
import { resolveConfig } from './resolve'

const defaultKeybindingsText = readFileSync(
  new URL('./defaults/keybindings.jsonc', import.meta.url),
  'utf8'
)
const defaultSettingsText = readFileSync(
  new URL('./defaults/settings.jsonc', import.meta.url),
  'utf8'
)

function resolve(userSettingsText: string | null, userKeybindingsText: string | null) {
  return resolveConfig({
    defaultKeybindingsText,
    userSettingsText,
    userKeybindingsText,
    systemPrefersDark: true
  })
}

describe('the shipped defaults', () => {
  it('says exactly what the app falls back to', () => {
    const parsed = parseJsonc(defaultSettingsText, 'settings.json')
    expect(parsed.problems).toEqual([])
    expect(parsed.value).toEqual(DEFAULT_SETTINGS)
  })

  it('binds only commands that exist', () => {
    const parsed = parseJsonc(defaultKeybindingsText, 'keybindings.json')
    expect(parsed.problems).toEqual([])

    let bindings: Record<string, unknown> = {}
    if (parsed.value !== null) {
      bindings = parsed.value
    }

    const commands = Object.values(bindings)
    const unknown = commands.filter((command) => !isCommandId(String(command)))
    expect(unknown).toEqual([])
  })
})

describe('resolveConfig', () => {
  it('lets a user file rebind a chord and reports what it could not use', () => {
    const user = '{\n  // mine\n  "cmd+shift+d": "terminal.split",\n  "cmd+": "terminal.new"\n}'
    const config = resolve(null, user)
    const bindings = new Map(config.bindings)

    expect(bindings.get('cmd+shift+d')).toBe('terminal.split')
    expect(bindings.get('cmd+\\')).toBe('terminal.split')
    expect(config.problems).toEqual(['"cmd+" is not a valid chord'])
  })

  it('resolves "system" against the OS and lets a named theme ignore it', () => {
    const fromSystem = resolve('{ "theme": "system" }', null)
    expect(fromSystem.appearance).toBe('dark')

    const pinned = resolve('{ "theme": "light" }', null)
    expect(pinned.appearance).toBe('light')
  })

  it('keeps a broken settings file from touching the keymap', () => {
    const config = resolve('{ "fontSize": }', '{ "cmd+shift+d": "terminal.split" }')
    const bindings = new Map(config.bindings)

    expect(config.settings).toEqual(DEFAULT_SETTINGS)
    expect(bindings.get('cmd+shift+d')).toBe('terminal.split')
    expect(config.problems).toHaveLength(1)
    expect(config.problems[0]).toContain('settings.json')
  })
})
