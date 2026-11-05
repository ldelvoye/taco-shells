import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { UpdateChecker } from './checker'

describe('UpdateChecker.checkNow', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('checks at once, then exactly once more after the 60 second GitHub cache', async () => {
    const noRelease = new Response('{}')
    const fetchImpl = vi.fn(() => Promise.resolve(noRelease.clone()))
    const checker = new UpdateChecker({
      feedUrl: 'http://127.0.0.1/feed',
      currentVersion: '1.0.0',
      updatesDirectory: '/nonexistent',
      fetchImpl,
      events: { onReady: () => undefined, onProblem: () => undefined }
    })

    checker.checkNow()
    expect(fetchImpl).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(60_000)
    expect(fetchImpl).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(15_000)
    expect(fetchImpl).toHaveBeenCalledTimes(2)

    await vi.advanceTimersByTimeAsync(600_000)
    expect(fetchImpl).toHaveBeenCalledTimes(2)
  })
})
