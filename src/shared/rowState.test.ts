import { describe, expect, it } from 'vitest'

import type { AgentRows } from './rowState'
import {
  rowsAfterExit,
  rowsAfterIdleTitle,
  rowsAfterScreen,
  rowsAfterSignal,
  summaryState,
  waitingCount
} from './rowState'

const A = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'
const B = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'
const empty: AgentRows = new Map()
const started = rowsAfterSignal(empty, 'p1', 'claude', A,'start', false)

describe('row state', () => {
  it('marks a stop done only when the pane is off screen', () => {
    const working = rowsAfterSignal(started, 'p1', 'claude', A,'working', false)
    expect(rowsAfterSignal(working, 'p1', 'claude', A,'stopped', false).get('p1')?.state).toBe('done')
    expect(rowsAfterSignal(working, 'p1', 'claude', A,'stopped', true).get('p1')?.state).toBe('idle')
  })

  it('ignores events from any session but the current one', () => {
    const restarted = rowsAfterSignal(started, 'p1', 'claude', B,'start', false)
    expect(rowsAfterSignal(restarted, 'p1', 'claude', A,'end', false)).toBe(restarted)
    expect(rowsAfterSignal(restarted, 'p1', 'claude', A,'working', false)).toBe(restarted)
    expect(rowsAfterSignal(empty, 'p1', 'claude', A,'working', false)).toBe(empty)
    expect(rowsAfterSignal(restarted, 'p1', 'claude', B,'end', false).has('p1')).toBe(false)
  })

  it('keeps the state when the current session starts again', () => {
    const working = rowsAfterSignal(started, 'p1', 'claude', A,'working', false)
    expect(rowsAfterSignal(working, 'p1', 'claude', A,'start', false)).toBe(working)
  })

  it('knows a session has a conversation once it is resumed or takes a prompt', () => {
    expect(started.get('p1')?.hasConversation).toBe(false)

    const prompted = rowsAfterSignal(started, 'p1', 'claude', A,'working', false)
    expect(prompted.get('p1')?.hasConversation).toBe(true)
    // the row is rebuilt on these paths, and must not forget it
    const stopped = rowsAfterSignal(prompted, 'p1', 'claude', A,'stopped', false)
    expect(rowsAfterScreen(stopped, ['p1']).get('p1')?.hasConversation).toBe(true)
    expect(rowsAfterIdleTitle(prompted, 'p1').get('p1')?.hasConversation).toBe(true)

    const resumed = rowsAfterSignal(empty, 'p1', 'claude', A,'resume', false)
    expect(resumed.get('p1')).toEqual({
      harness: 'claude',
      sessionId: A,
      state: 'idle',
      hasConversation: true
    })
    expect(rowsAfterSignal(prompted, 'p1', 'claude', A,'resume', false)).toBe(prompted)
  })

  it('clears done when the pane comes on screen', () => {
    const done = rowsAfterSignal(started, 'p1', 'claude', A,'stopped', false)
    expect(rowsAfterScreen(done, ['p1']).get('p1')?.state).toBe('idle')
    expect(rowsAfterScreen(done, ['p2'])).toBe(done)
  })

  it('lets an idle title clear working and needs but nothing else', () => {
    const needs = rowsAfterSignal(started, 'p1', 'claude', A,'needs', false)
    const done = rowsAfterSignal(started, 'p1', 'claude', A,'stopped', false)
    expect(rowsAfterIdleTitle(needs, 'p1').get('p1')?.state).toBe('idle')
    expect(rowsAfterIdleTitle(done, 'p1')).toBe(done)
  })

  it('summarises a group by its most urgent pane, and by nothing when all are idle', () => {
    const withP2 = rowsAfterSignal(started, 'p2', 'claude', B,'start', false)
    const idle = ['p1', 'p2']
    expect(summaryState(withP2, idle)).toBeNull()
    expect(summaryState(withP2, ['p3'])).toBeNull()

    const done = rowsAfterSignal(withP2, 'p1', 'claude', A,'stopped', false)
    expect(summaryState(done, idle)).toBe('done')

    const working = rowsAfterSignal(done, 'p2', 'claude', B,'working', false)
    expect(summaryState(working, idle)).toBe('working')

    const needs = rowsAfterSignal(working, 'p1', 'claude', A,'needs', false)
    expect(summaryState(needs, idle)).toBe('needs')
  })

  it('counts the panes waiting on the user', () => {
    const needs = rowsAfterSignal(started, 'p1', 'claude', A,'needs', false)
    expect(waitingCount(needs)).toBe(1)
    expect(waitingCount(rowsAfterExit(needs, 'p1'))).toBe(0)
  })
})
