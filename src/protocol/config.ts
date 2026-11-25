import type { CommandId } from './commands'
import type { Settings } from './settings'
import type { Appearance } from './theme'

export type BindingEntry = [chord: string, command: CommandId]

export type ConfigFile = 'settings' | 'keybindings'

export interface Config {
  settings: Settings
  appearance: Appearance
  bindings: BindingEntry[]
  problems: string[]
}
