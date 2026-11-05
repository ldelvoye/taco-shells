import type { RowColor } from './model'

export const COMMAND_IDS = [
  'terminal.new',
  'terminal.close',
  'terminal.split',
  'terminal.focusNextPane',
  'terminal.focusPreviousPane',
  'terminal.focusNextGroup',
  'terminal.focusPreviousGroup',
  'terminal.copy',
  'terminal.paste',
  'terminal.clear',
  'terminal.killLine',
  'terminal.lineStart',
  'terminal.lineEnd',
  'terminal.insertNewline',
  'terminal.colorRed',
  'terminal.colorOrange',
  'terminal.colorYellow',
  'terminal.colorGreen',
  'terminal.colorBlue',
  'terminal.colorPurple',
  'sidebar.toggle',
  'config.openSettings',
  'config.openKeybindings'
] as const

export type CommandId = (typeof COMMAND_IDS)[number]

export const ROW_COLOR_COMMANDS: Record<RowColor, CommandId> = {
  red: 'terminal.colorRed',
  orange: 'terminal.colorOrange',
  yellow: 'terminal.colorYellow',
  green: 'terminal.colorGreen',
  blue: 'terminal.colorBlue',
  purple: 'terminal.colorPurple'
}

export function isCommandId(value: string): value is CommandId {
  const knownIds: readonly string[] = COMMAND_IDS
  return knownIds.includes(value)
}
