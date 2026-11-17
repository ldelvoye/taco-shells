import { type ClaudeSession, isSessionUuid } from '@shared/snapshot'

const UNTYPEABLE_CHARACTERS = /[\p{Cc}\p{Zl}\p{Zp}]/gu

// A line typed before the shell's prompt is up is cut at 1024 bytes, losing the
// closing quote. 200 characters stay under that at four bytes each.
const MAX_NAME_CHARACTERS = 200

/** The line that resumes a Claude session in a shell, or null when its id is not a UUID. */
export function resumeCommand(session: ClaudeSession): string | null {
  if (!isSessionUuid(session.sessionId)) {
    return null
  }
  return `claude --resume ${session.sessionId}\n`
}

/** The line that forks a Claude session under a name, or null when its id is not a UUID. */
export function forkCommand(session: ClaudeSession, name: string): string | null {
  if (!isSessionUuid(session.sessionId)) {
    return null
  }
  const base = `claude --resume ${session.sessionId} --fork-session`
  const typeableName = name.replace(UNTYPEABLE_CHARACTERS, '')
  const characters = Array.from(typeableName)
  const keptCharacters = characters.slice(0, MAX_NAME_CHARACTERS)
  const cleanName = keptCharacters.join('')
  if (cleanName === '') {
    return `${base}\n`
  }
  const escapedName = cleanName.replaceAll("'", "'\\''")
  return `${base} --name '${escapedName}'\n`
}
