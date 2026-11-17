import { describe, expect, it } from 'vitest'
import { forkCommand, resumeCommand } from './resume'
import { shellStartFor } from './start'

describe('shellStartFor', () => {
  it('starts a fork where its session lives, or as a plain split when it has none', () => {
    const known = { sessionId: '123e4567-e89b-12d3-a456-426614174000', cwd: '/work' }
    const lookup = (id: string) => {
      if (id === 'pty-1') {
        return known
      }
      return null
    }

    expect(shellStartFor({ fork: { from: 'pty-1', name: '⑂ x' } }, lookup)).toEqual({
      cwd: '/work',
      cwdFrom: null,
      command: forkCommand(known, '⑂ x')
    })
    expect(shellStartFor({ fork: { from: 'pty-9', name: '⑂ x' } }, lookup)).toEqual({
      cwd: null,
      cwdFrom: 'pty-9',
      command: null
    })
    expect(shellStartFor({ resume: known }, lookup)).toEqual({
      cwd: '/work',
      cwdFrom: null,
      command: resumeCommand(known)
    })
    expect(shellStartFor({ cwdFrom: 'pty-1' }, lookup)).toEqual({
      cwd: null,
      cwdFrom: 'pty-1',
      command: null
    })
    expect(shellStartFor({}, lookup)).toEqual({ cwd: null, cwdFrom: null, command: null })
  })
})
