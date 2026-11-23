import { describe, expect, it } from 'vitest'
import { claudeForkCommand, claudeResumeCommand } from './commands'

describe('claudeResumeCommand', () => {
  it('types the resume line for a real session id and refuses anything else', () => {
    const valid = '123e4567-e89b-12d3-a456-426614174000'
    const injected = '$(rm -rf /)'

    expect(claudeResumeCommand(valid)).toBe('claude --resume 123e4567-e89b-12d3-a456-426614174000\n')
    expect(claudeResumeCommand(injected)).toBeNull()
  })
})

describe('claudeForkCommand', () => {
  it('types the fork line with the name as one quoted argument', () => {
    const sessionId = '123e4567-e89b-12d3-a456-426614174000'
    const resume = 'claude --resume 123e4567-e89b-12d3-a456-426614174000 --fork-session'

    expect(claudeForkCommand(sessionId, '⑂ Tacos fact')).toBe(`${resume} --name '⑂ Tacos fact'\n`)
    const injected = claudeForkCommand(sessionId, "it's $(rm -rf /)")
    expect(injected).toBe(`${resume} --name 'it'\\''s $(rm -rf /)'\n`)
    // control characters would be read as keys by the shell's line editor
    expect(claudeForkCommand(sessionId, 'a\x1b[2Jb\nc\x07')).toBe(`${resume} --name 'a[2Jbc'\n`)
    // a longer line is cut by the tty before the shell reads it
    const longest = claudeForkCommand(sessionId, "😀'".repeat(1000)) as string
    expect(new TextEncoder().encode(longest).length).toBeLessThan(1024)
    expect(longest.endsWith("'\n")).toBe(true)
    expect(claudeForkCommand(sessionId, '')).toBe(`${resume}\n`)
    expect(claudeForkCommand('$(rm -rf /)', 'x')).toBeNull()
  })
})
