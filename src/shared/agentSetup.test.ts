import { describe, expect, it } from 'vitest'
import { agentBannerOf, agentMenuOf, agentSetupStateOf } from './agentSetup'
import type { AgentNotice, AgentSetupFacts, AgentSetupState } from './agentSetup'

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

  it('offers only to an agent that is installed, unwired, never declined, in a build that may write', () => {
    const states: AgentSetupState[] = ['notInstalled', 'off', 'byHand', 'setUp', 'declined', 'notSetUp']

    for (const state of states) {
      const banner = agentBannerOf('claude', 'Claude Code', state, true, null)
      if (state === 'notSetUp') {
        expect(banner?.kind).toBe('offer')
      } else {
        expect(banner).toBeNull()
      }
    }
    expect(agentBannerOf('claude', 'Claude Code', 'notSetUp', false, null)).toBeNull()

    const problem: AgentNotice = { kind: 'problem', text: 'nothing was changed' }
    for (const state of states) {
      const banner = agentBannerOf('claude', 'Claude Code', state, true, problem)
      expect(banner).toEqual({ harness: 'claude', kind: 'problem', text: 'nothing was changed' })
    }

    const done = agentBannerOf('claude', 'Claude Code', 'setUp', true, { kind: 'done' })
    expect(done?.kind).toBe('done')
  })

  it('stops saying an agent is set up once it no longer is', () => {
    const done: AgentNotice = { kind: 'done' }

    const undoneByHand = agentBannerOf('claude', 'Claude Code', 'notSetUp', true, done)
    expect(undoneByHand?.kind).toBe('offer')

    for (const state of ['declined', 'off', 'notInstalled'] as const) {
      expect(agentBannerOf('claude', 'Claude Code', state, true, done)).toBeNull()
    }
  })
})
