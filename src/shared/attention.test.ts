import { describe, expect, it } from 'vitest'

import { waitingAfterAttention, waitingAfterExit, waitingAfterFocus } from './attention'

describe('the pane a raise jumps to', () => {
  it('ignores a session asking while you are already in its pane', () => {
    expect(waitingAfterAttention(null, 'pty-1', 'pty-1')).toBe(null)
    expect(waitingAfterAttention('pty-2', 'pty-1', 'pty-1')).toBe('pty-2')
  })

  it('takes the most recent asker, and forgets it once you arrive', () => {
    const firstAsk = waitingAfterAttention(null, 'pty-1', 'pty-3')
    expect(firstAsk).toBe('pty-1')

    const secondAsk = waitingAfterAttention(firstAsk, 'pty-2', 'pty-3')
    expect(secondAsk).toBe('pty-2')

    expect(waitingAfterFocus(secondAsk, 'pty-2')).toBe(null)
    expect(waitingAfterFocus(secondAsk, 'pty-3')).toBe('pty-2')
  })

  it('forgets a pane whose shell exits', () => {
    expect(waitingAfterExit('pty-1', 'pty-1')).toBe(null)
    expect(waitingAfterExit('pty-1', 'pty-2')).toBe('pty-1')
  })
})
