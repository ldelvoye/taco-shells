import { describe, expect, it } from 'vitest'
import { claudeForkCommand, claudeResumeCommand } from '@shared/harness/claude/commands'
import type { Harness } from '@shared/harness/types'
import { claude } from '@shared/harness/claude'
import { shellStartFor } from './start'

const noFork: Harness = {
  id: 'nofork',
  name: 'No Fork',
  isSessionId: (value): value is string => typeof value === 'string',
  signalFor: () => null,
  resumeCommand: (sessionId) => `nofork resume ${sessionId}\n`
}

describe('shellStartFor', () => {
  it('starts a fork where its session lives, or as a plain split when it has none', () => {
    const known = { harness: 'claude', sessionId: '123e4567-e89b-12d3-a456-426614174000', cwd: '/work' }
    const lookup = (id: string) => {
      if (id === 'pty-1') {
        return known
      }
      return null
    }

    expect(shellStartFor({ fork: { from: 'pty-1', name: '⑂ x' } }, lookup)).toEqual({
      cwd: '/work',
      cwdFrom: null,
      command: claudeForkCommand(known.sessionId, '⑂ x')
    })
    expect(shellStartFor({ fork: { from: 'pty-9', name: '⑂ x' } }, lookup)).toEqual({
      cwd: null,
      cwdFrom: 'pty-9',
      command: null
    })
    expect(shellStartFor({ resume: known }, lookup)).toEqual({
      cwd: '/work',
      cwdFrom: null,
      command: claudeResumeCommand(known.sessionId)
    })
    expect(shellStartFor({ cwdFrom: 'pty-1' }, lookup)).toEqual({
      cwd: null,
      cwdFrom: 'pty-1',
      command: null
    })
    expect(shellStartFor({}, lookup)).toEqual({ cwd: null, cwdFrom: null, command: null })
  })

  it("resumes with the session's own harness, and opens a plain shell when it is gone or cannot fork", () => {
    const list = [claude, noFork]
    const gone = { harness: 'gone', sessionId: 'x1', cwd: '/work' }
    const stuck = { harness: 'nofork', sessionId: 'x2', cwd: '/other' }
    const lookup = () => stuck

    expect(shellStartFor({ resume: stuck }, lookup, list)).toEqual({
      cwd: '/other',
      cwdFrom: null,
      command: 'nofork resume x2\n'
    })
    expect(shellStartFor({ resume: gone }, lookup, list)).toEqual({
      cwd: '/work',
      cwdFrom: null,
      command: null
    })
    expect(shellStartFor({ fork: { from: 'pty-1', name: '⑂ x' } }, lookup, list)).toEqual({
      cwd: null,
      cwdFrom: 'pty-1',
      command: null
    })
  })
})
