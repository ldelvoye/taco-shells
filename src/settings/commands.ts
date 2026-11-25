import type { RowColor } from '@protocol/arrangement'
import { COMMAND_IDS } from '@protocol/commands'
import type { CommandId } from '@protocol/commands'

export const ROW_COLOR_COMMANDS: Record<RowColor, CommandId> = {
  red: 'terminal.colorRed',
  orange: 'terminal.colorOrange',
  yellow: 'terminal.colorYellow',
  green: 'terminal.colorGreen',
  blue: 'terminal.colorBlue',
  purple: 'terminal.colorPurple'
}

// Commands a held key keeps firing. Every other command is a discrete action
// and runs once per press.
export const REPEATING_COMMANDS: ReadonlySet<CommandId> = new Set<CommandId>([
  'terminal.wordLeft',
  'terminal.wordRight'
])

export function isCommandId(value: string): value is CommandId {
  const knownIds: readonly string[] = COMMAND_IDS
  return knownIds.includes(value)
}
