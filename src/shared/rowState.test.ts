import { describe, expect, it } from 'vitest'

import type { ClaudeRows } from './rowState'
import {
  rowsAfterExit,
  rowsAfterIdleTitle,
  rowsAfterScreen,
  rowsAfterSignal,
  signalForHook,
  waitingCount
} from './rowState'

const A = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'
const B = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'
const empty: ClaudeRows = new Map()
const started = rowsAfterSignal(empty, 'p1', A, 'start', false)

describe('row state', () => {
  it('maps hook events to signals, ignoring the idle reminder and subagents', () => {
    expect(signalForHook('UserPromptSubmit', null)).toBe('working')
    expect(signalForHook('PreToolUse', 'Bash')).toBe('working')
    expect(signalForHook('PreToolUse', 'AskUserQuestion')).toBe('needs')
    expect(signalForHook('Notification', 'permission_prompt')).toBe('needs')
    expect(signalForHook('Notification', 'idle_prompt')).toBeNull()
    expect(signalForHook('Notification', null)).toBeNull()
    expect(signalForHook('Stop', null)).toBe('stopped')
    expect(signalForHook('StopFailure', null)).toBe('stopped')
    expect(signalForHook('SubagentStop', null)).toBeNull()
  })

  it('marks a stop done only when the pane is off screen', () => {
    const working = rowsAfterSignal(started, 'p1', A, 'working', false)
    expect(rowsAfterSignal(working, 'p1', A, 'stopped', false).get('p1')?.state).toBe('done')
    expect(rowsAfterSignal(working, 'p1', A, 'stopped', true).get('p1')?.state).toBe('idle')
  })

  it('ignores events from any session but the current one', () => {
    const restarted = rowsAfterSignal(started, 'p1', B, 'start', false)
    expect(rowsAfterSignal(restarted, 'p1', A, 'end', false)).toBe(restarted)
    expect(rowsAfterSignal(restarted, 'p1', A, 'working', false)).toBe(restarted)
    expect(rowsAfterSignal(empty, 'p1', A, 'working', false)).toBe(empty)
    expect(rowsAfterSignal(restarted, 'p1', B, 'end', false).has('p1')).toBe(false)
  })

  it('keeps the state when the current session starts again', () => {
    const working = rowsAfterSignal(started, 'p1', A, 'working', false)
    expect(rowsAfterSignal(working, 'p1', A, 'start', false)).toBe(working)
  })

  it('clears done when the pane comes on screen', () => {
    const done = rowsAfterSignal(started, 'p1', A, 'stopped', false)
    expect(rowsAfterScreen(done, ['p1']).get('p1')?.state).toBe('idle')
    expect(rowsAfterScreen(done, ['p2'])).toBe(done)
  })

  it('lets an idle title clear working and needs but nothing else', () => {
    const needs = rowsAfterSignal(started, 'p1', A, 'needs', false)
    const done = rowsAfterSignal(started, 'p1', A, 'stopped', false)
    expect(rowsAfterIdleTitle(needs, 'p1').get('p1')?.state).toBe('idle')
    expect(rowsAfterIdleTitle(done, 'p1')).toBe(done)
  })

  it('counts the panes waiting on the user', () => {
    const needs = rowsAfterSignal(started, 'p1', A, 'needs', false)
    expect(waitingCount(needs)).toBe(1)
    expect(waitingCount(rowsAfterExit(needs, 'p1'))).toBe(0)
  })
})
