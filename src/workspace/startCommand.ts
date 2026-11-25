import { HARNESSES, harnessOf } from '@agents/registry'
import type { Harness } from '@agents/types'
import type { SessionId } from '@protocol/ipc'
import type { AgentSession } from './snapshot/format'

export interface ForkRequest {
  from: SessionId
  name: string
}

export interface PtyCreateOptions {
  cwdFrom?: SessionId
  cwd?: string
  resume?: AgentSession
  fork?: ForkRequest
}

export interface ShellStart {
  cwd: string | null
  cwdFrom: SessionId | null
  command: string | null
}

/** Where a new shell starts and what is typed into it; a resume or fork its harness cannot run is a plain shell. */
export function shellStartFor(
  options: PtyCreateOptions,
  agentSessionOf: (id: SessionId) => AgentSession | null,
  harnesses: readonly Harness[] = HARNESSES
): ShellStart {
  if (options.resume !== undefined) {
    const harness = harnessOf(options.resume.harness, harnesses)
    if (harness === null) {
      return { cwd: options.resume.cwd, cwdFrom: null, command: null }
    }
    const command = harness.resumeCommand(options.resume.sessionId)
    return { cwd: options.resume.cwd, cwdFrom: null, command }
  }

  if (options.fork !== undefined) {
    const source = agentSessionOf(options.fork.from)
    if (source === null) {
      return { cwd: null, cwdFrom: options.fork.from, command: null }
    }
    const harness = harnessOf(source.harness, harnesses)
    if (harness === null || harness.forkCommand === undefined) {
      return { cwd: null, cwdFrom: options.fork.from, command: null }
    }
    const command = harness.forkCommand(source.sessionId, options.fork.name)
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
