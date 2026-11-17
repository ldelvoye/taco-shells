import { describe, expect, it } from 'vitest'
import { forkCommand, resumeCommand } from './resume'

describe('resumeCommand', () => {
  it('types the resume line for a real session id and refuses anything else', () => {
    const valid = { sessionId: '123e4567-e89b-12d3-a456-426614174000', cwd: '/tmp' }
    const injected = { sessionId: '$(rm -rf /)', cwd: '/tmp' }

    expect(resumeCommand(valid)).toBe('claude --resume 123e4567-e89b-12d3-a456-426614174000\n')
    expect(resumeCommand(injected)).toBeNull()
  })
})

describe('forkCommand', () => {
  it('types the fork line with the name as one quoted argument', () => {
    const session = { sessionId: '123e4567-e89b-12d3-a456-426614174000', cwd: '/tmp' }
    const resume = 'claude --resume 123e4567-e89b-12d3-a456-426614174000 --fork-session'

    expect(forkCommand(session, '⑂ Tacos fact')).toBe(`${resume} --name '⑂ Tacos fact'\n`)
    const injected = forkCommand(session, "it's $(rm -rf /)")
    expect(injected).toBe(`${resume} --name 'it'\\''s $(rm -rf /)'\n`)
    // control characters would be read as keys by the shell's line editor
    expect(forkCommand(session, 'a\x1b[2Jb\nc\x07')).toBe(`${resume} --name 'a[2Jbc'\n`)
    // a longer line is cut by the tty before the shell reads it
    const longest = forkCommand(session, "😀'".repeat(1000)) as string
    expect(Buffer.byteLength(longest)).toBeLessThan(1024)
    expect(longest.endsWith("'\n")).toBe(true)
    expect(forkCommand(session, '')).toBe(`${resume}\n`)
    expect(forkCommand({ sessionId: '$(rm -rf /)', cwd: '/tmp' }, 'x')).toBeNull()
  })
})
