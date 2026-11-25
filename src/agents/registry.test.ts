import { describe, expect, it } from 'vitest'
import { claude } from './claude'
import { agentTitleOf } from './registry'
import type { Harness } from './types'

const bare: Harness = {
  id: 'bare',
  name: 'Bare',
  isSessionId: (value): value is string => typeof value === 'string',
  signalFor: () => null,
  resumeCommand: () => null
}

describe('agentTitleOf', () => {
  it("reads a title with the row's harness, and leaves it alone without a reader", () => {
    const list = [claude, bare]

    expect(agentTitleOf('claude', '✳ Tacos', list)).toEqual({ name: 'Tacos', idle: true })
    expect(agentTitleOf('bare', '✳ Tacos', list)).toEqual({ name: '✳ Tacos', idle: false })
    expect(agentTitleOf('gone', '✳ Tacos', list)).toEqual({ name: '✳ Tacos', idle: false })
  })
})
