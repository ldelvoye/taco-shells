import { describe, expect, it } from 'vitest'
import { resumeCommand } from './resume'

describe('resumeCommand', () => {
  it('types the resume line for a real session id and refuses anything else', () => {
    const valid = { sessionId: '123e4567-e89b-12d3-a456-426614174000', cwd: '/tmp' }
    const injected = { sessionId: '$(rm -rf /)', cwd: '/tmp' }

    expect(resumeCommand(valid)).toBe('claude --resume 123e4567-e89b-12d3-a456-426614174000\n')
    expect(resumeCommand(injected)).toBeNull()
  })
})
