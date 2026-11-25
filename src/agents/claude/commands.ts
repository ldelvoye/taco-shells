import { isClaudeSessionId } from './sessionId'

const UNTYPEABLE_CHARACTERS = /[\p{Cc}\p{Zl}\p{Zp}]/gu

// A line typed before the shell's prompt is up is cut at 1024 bytes, losing the
// closing quote. 200 characters stay under that at four bytes each.
const MAX_NAME_CHARACTERS = 200

/** The line that resumes a Claude session in a shell, or null when its id is not a UUID. */
export function claudeResumeCommand(sessionId: string): string | null {
  if (!isClaudeSessionId(sessionId)) {
    return null
  }
  return `claude --resume ${sessionId}\n`
}

/** The line that forks a Claude session under a name, or null when its id is not a UUID. */
export function claudeForkCommand(sessionId: string, name: string): string | null {
  if (!isClaudeSessionId(sessionId)) {
    return null
  }
  const base = `claude --resume ${sessionId} --fork-session`
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
