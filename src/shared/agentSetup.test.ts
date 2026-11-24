import { describe, expect, it } from 'vitest'
import { agentMenuOf, agentSetupStateOf } from './agentSetup'
import type { AgentSetupFacts, AgentSetupState } from './agentSetup'

describe('agent setup', () => {
  it('reads the state in order of precedence', () => {
    const rows: { facts: AgentSetupFacts; state: AgentSetupState }[] = [
      {
        facts: { installed: false, managing: true, wiring: 'managed', declined: true },
        state: 'notInstalled'
      },
      {
        facts: { installed: true, managing: false, wiring: 'managed', declined: false },
        state: 'off'
      },
      {
        facts: { installed: true, managing: true, wiring: 'byHand', declined: true },
        state: 'byHand'
      },
      {
        facts: { installed: true, managing: true, wiring: 'byHand', declined: false },
        state: 'byHand'
      },
      {
        facts: { installed: true, managing: true, wiring: 'managed', declined: true },
        state: 'setUp'
      },
      {
        facts: { installed: true, managing: true, wiring: 'none', declined: true },
        state: 'declined'
      },
      {
        facts: { installed: true, managing: true, wiring: 'none', declined: false },
        state: 'notSetUp'
      }
    ]

    for (const row of rows) {
      expect(agentSetupStateOf(row.facts)).toBe(row.state)
    }
  })

  it('only lets a build that may write act', () => {
    const states: AgentSetupState[] = ['notInstalled', 'off', 'byHand', 'setUp', 'declined', 'notSetUp']
    const alwaysDisabled: AgentSetupState[] = ['notInstalled', 'off']

    for (const state of states) {
      const writable = agentMenuOf(state, 'Claude Code', true)
      const readOnly = agentMenuOf(state, 'Claude Code', false)
      const expectedWhenWritable = !alwaysDisabled.includes(state)

      expect(writable.enabled).toBe(expectedWhenWritable)
      expect(readOnly.enabled).toBe(false)
    }

    expect(agentMenuOf('byHand', 'Claude Code', true).action).toBe('switch')
    expect(agentMenuOf('setUp', 'Claude Code', true).action).toBe('remove')
    expect(agentMenuOf('notSetUp', 'Claude Code', true).actionLabel).toBe('Set Up Claude Code')
  })
})
