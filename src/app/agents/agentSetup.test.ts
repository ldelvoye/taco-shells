import { describe, expect, it } from 'vitest'
import { agentBannerOf, agentMenuOf, agentNoticeAfter, agentSetupStateOf, runsAgent } from './agentSetup'
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
      expect(writable.note).toBeNull()
      expect(readOnly.enabled).toBe(false)
      // The status line already explains the always-disabled states; only the build's own refusal needs a note.
      const hasNote = readOnly.note !== null
      expect(hasNote).toBe(expectedWhenWritable)
    }

    expect(agentMenuOf('byHand', 'Claude Code', true).action).toBe('switch')
    expect(agentMenuOf('setUp', 'Claude Code', true).action).toBe('remove')
  })

  it('offers only to an agent that is installed, unwired, never declined, in a build that may write', () => {
    const states: AgentSetupState[] = ['notInstalled', 'off', 'byHand', 'setUp', 'declined', 'notSetUp']

    for (const state of states) {
      const banner = agentBannerOf('claude', 'Claude Code', state, true, null, true)
      if (state === 'notSetUp') {
        expect(banner?.kind).toBe('offer')
      } else {
        expect(banner).toBeNull()
      }
    }
    expect(agentBannerOf('claude', 'Claude Code', 'notSetUp', false, null, true)).toBeNull()
    expect(agentBannerOf('claude', 'Claude Code', 'notSetUp', true, null, false)).toBeNull()

    const problem: AgentNotice = { kind: 'problem', text: 'nothing was changed' }
    for (const state of states) {
      const banner = agentBannerOf('claude', 'Claude Code', state, true, problem, false)
      expect(banner).toEqual({ harness: 'claude', kind: 'problem', text: 'nothing was changed' })
    }

    const done = agentBannerOf('claude', 'Claude Code', 'setUp', true, { kind: 'done' }, true)
    expect(done?.kind).toBe('done')
  })

  it('leaves a done line after setting up, nothing after a removal, and the problem after a failure', () => {
    expect(agentNoticeAfter('setUp', null)).toEqual({ kind: 'done' })
    expect(agentNoticeAfter('switch', null)).toEqual({ kind: 'done' })
    expect(agentNoticeAfter('remove', null)).toBeNull()
    expect(agentNoticeAfter('remove', 'it broke')).toEqual({ kind: 'problem', text: 'it broke' })
  })

  it('stops saying an agent is set up once it no longer is', () => {
    const done: AgentNotice = { kind: 'done' }

    const undoneByHand = agentBannerOf('claude', 'Claude Code', 'notSetUp', true, done, true)
    expect(undoneByHand?.kind).toBe('offer')

    for (const state of ['declined', 'off', 'notInstalled'] as const) {
      expect(agentBannerOf('claude', 'Claude Code', state, true, done, true)).toBeNull()
    }
  })

  it('recognises the agent among a terminal\'s foreground programs', () => {
    const commands = ['claude']
    const cases: { lines: string[]; expected: boolean }[] = [
      { lines: ['claude --resume abc'], expected: true },
      { lines: ['/Users/a/.local/bin/claude'], expected: true },
      { lines: ['node /opt/homebrew/bin/claude'], expected: true },
      { lines: ['-zsh'], expected: false },
      { lines: ['vim claude'], expected: false },
      { lines: ['claude-monitor'], expected: false },
      { lines: ['node server.js claude'], expected: false },
      { lines: ['railway mcp proxy', 'claude'], expected: true },
      { lines: [], expected: false }
    ]

    for (const row of cases) {
      expect(runsAgent(row.lines, commands)).toBe(row.expected)
    }
  })
})
