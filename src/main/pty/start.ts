import type { PtyCreateOptions, SessionId } from '@shared/ipc'
import type { ClaudeSession } from '@shared/snapshot'
import { forkCommand, resumeCommand } from './resume'

export interface ShellStart {
  cwd: string | null
  cwdFrom: SessionId | null
  command: string | null
}

/** Where a new shell starts and what is typed into it; a fork with no known session is a plain split. */
export function shellStartFor(
  options: PtyCreateOptions,
  claudeSessionOf: (id: SessionId) => ClaudeSession | null
): ShellStart {
  if (options.resume !== undefined) {
    const command = resumeCommand(options.resume)
    return { cwd: options.resume.cwd, cwdFrom: null, command }
  }

  if (options.fork !== undefined) {
    const source = claudeSessionOf(options.fork.from)
    if (source === null) {
      return { cwd: null, cwdFrom: options.fork.from, command: null }
    }
    const command = forkCommand(source, options.fork.name)
    return { cwd: source.cwd, cwdFrom: null, command }
  }

  if (options.cwd !== undefined) {
    return { cwd: options.cwd, cwdFrom: null, command: null }
  }

  if (options.cwdFrom !== undefined) {
    return { cwd: null, cwdFrom: options.cwdFrom, command: null }
  }

  return { cwd: null, cwdFrom: null, command: null }
}
