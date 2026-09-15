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
  'terminal.insertNewline',
  'sidebar.toggle',
  'config.openSettings',
  'config.openKeybindings'
] as const

export type CommandId = (typeof COMMAND_IDS)[number]

export function isCommandId(value: string): value is CommandId {
  const knownIds: readonly string[] = COMMAND_IDS
  return knownIds.includes(value)
}
