export const COMMAND_IDS = ['terminal.copy', 'terminal.paste', 'terminal.clear'] as const

export type CommandId = (typeof COMMAND_IDS)[number]

export function isCommandId(value: string): value is CommandId {
  const knownIds: readonly string[] = COMMAND_IDS
  return knownIds.includes(value)
}
