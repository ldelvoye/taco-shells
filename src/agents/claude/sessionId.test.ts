import { describe, expect, it } from 'vitest'

import { isClaudeSessionId } from './sessionId'

const SESSION_ID = '123e4567-e89b-12d3-a456-426614174000'

describe('isClaudeSessionId', () => {
  it('accepts a UUID in either case and rejects anything else', () => {
    expect(isClaudeSessionId(SESSION_ID)).toBe(true)
    expect(isClaudeSessionId(SESSION_ID.toUpperCase())).toBe(true)
    expect(isClaudeSessionId(`${SESSION_ID}\n`)).toBe(false)
    expect(isClaudeSessionId('--dangerously-skip-permissions')).toBe(false)
    expect(isClaudeSessionId(42)).toBe(false)
  })
})
