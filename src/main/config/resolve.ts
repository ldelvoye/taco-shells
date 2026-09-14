import type { BindingEntry, Config } from '@shared/config'
import { resolveKeymap, type Keymap } from '@shared/keys'
import { applySettingsLayer, DEFAULT_SETTINGS } from '@shared/settings'
import type { Appearance } from '@shared/theme'
import { parseJsonc } from './jsonc'

export interface ResolveInput {
  defaultKeybindingsText: string
  userSettingsText: string | null
  userKeybindingsText: string | null
  systemPrefersDark: boolean
}

// Spreading into push passes one argument per element, which overflows the stack
// on an array of a hundred thousand or so, and a config file full of mistakes
// reaches that.
function addProblems(problems: string[], found: string[]): void {
  for (const problem of found) {
    problems.push(problem)
  }
}

export function resolveConfig(input: ResolveInput): Config {
  const problems: string[] = []

  const defaultResult = parseJsonc(input.defaultKeybindingsText, 'keybindings.json')
  addProblems(problems, defaultResult.problems)

  let defaultKeymap: Keymap
  if (defaultResult.value !== null) {
    defaultKeymap = defaultResult.value as Keymap
  } else {
    defaultKeymap = {}
  }

  let settings = DEFAULT_SETTINGS
  if (input.userSettingsText !== null) {
    const userSettingsResult = parseJsonc(input.userSettingsText, 'settings.json')
    addProblems(problems, userSettingsResult.problems)

    if (userSettingsResult.value !== null) {
      const layer = applySettingsLayer(DEFAULT_SETTINGS, userSettingsResult.value, 'settings.json')
      settings = layer.settings
      addProblems(problems, layer.problems)
    }
  }

  let userKeymap: Keymap | undefined
  if (input.userKeybindingsText !== null) {
    const userKeybindingsResult = parseJsonc(input.userKeybindingsText, 'keybindings.json')
    addProblems(problems, userKeybindingsResult.problems)

    if (userKeybindingsResult.value !== null) {
      userKeymap = userKeybindingsResult.value as Keymap
    }
  }

  const resolution = resolveKeymap(defaultKeymap, userKeymap)
  const bindings: BindingEntry[] = Array.from(resolution.bindings)
  addProblems(problems, resolution.problems)

  let appearance: Appearance
  if (settings.theme === 'system') {
    if (input.systemPrefersDark) {
      appearance = 'dark'
    } else {
      appearance = 'light'
    }
  } else if (settings.theme === 'light') {
    appearance = 'light'
  } else {
    appearance = 'dark'
  }

  return { settings, appearance, bindings, problems }
}
