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

  it('gives up on a feed that never answers, so the recheck 75 seconds later still fetches', async () => {
    const stalledUntilAborted: typeof fetch = (_input, init) => {
      return new Promise((_resolve, reject) => {
        if (init === undefined || !init.signal) {
          return
        }
        init.signal.addEventListener('abort', () => {
          reject(new Error('aborted'))
        })
      })
    }
    const fetchImpl = vi.fn(stalledUntilAborted)
    const checker = new UpdateChecker({
      feedUrl: 'http://127.0.0.1/feed',
      currentVersion: '1.0.0',
      updatesDirectory: '/nonexistent',
      fetchImpl,
      events: { onReady: () => undefined, onProblem: () => undefined }
    })

    checker.checkNow()
    await vi.advanceTimersByTimeAsync(75_000)

    expect(fetchImpl).toHaveBeenCalledTimes(2)
    checker.stop()
  })
})
