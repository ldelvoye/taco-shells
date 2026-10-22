import { type ClaudeSession, isSessionUuid } from '@shared/snapshot'

/** The line that resumes a Claude session in a shell, or null when its id is not a UUID. */
export function resumeCommand(session: ClaudeSession): string | null {
  if (!isSessionUuid(session.sessionId)) {
    return null
  }
  return `claude --resume ${session.sessionId}\n`
}
