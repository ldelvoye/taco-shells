export const COMMAND_IDS = [
  'terminal.new',
  'terminal.close',
  'terminal.copy',
  'terminal.paste',
  'terminal.clear',
  'sidebar.toggle'
] as const

export type CommandId = (typeof COMMAND_IDS)[number]

export function isCommandId(value: string): value is CommandId {
  const knownIds: readonly string[] = COMMAND_IDS
  return knownIds.includes(value)
}
