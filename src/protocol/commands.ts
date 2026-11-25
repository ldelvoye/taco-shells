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
  'terminal.wordLeft',
  'terminal.wordRight',
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
