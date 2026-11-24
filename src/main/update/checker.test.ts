import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { UpdateChecker } from './checker'
import { assetNameFor } from './feed'

const FEED_URL = 'http://127.0.0.1/feed'
const ASSET_URL = 'http://127.0.0.1/asset.zip'
const CURRENT_VERSION = '1.0.0'

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

describe('UpdateChecker release choice', () => {
  let updatesDirectory = ''

  beforeEach(() => {
    vi.useFakeTimers()
    updatesDirectory = mkdtempSync(join(tmpdir(), 'taco-shells-checker-test-'))
  })

  afterEach(() => {
    vi.useRealTimers()
    rmSync(updatesDirectory, { recursive: true, force: true })
  })

  async function urlsFetchedWhenFeedOffers(feedVersion: string): Promise<string[]> {
    const payload = {
      tag_name: `v${feedVersion}`,
      prerelease: false,
      assets: [
        { name: assetNameFor(feedVersion), size: 100, browser_download_url: ASSET_URL }
      ]
    }
    const fetched: string[] = []
    const fetchImpl: typeof fetch = (input) => {
      const url = String(input)
      fetched.push(url)
      if (url === FEED_URL) {
        return Promise.resolve(new Response(JSON.stringify(payload)))
      }
      return Promise.resolve(new Response('', { status: 404 }))
    }
    const checker = new UpdateChecker({
      feedUrl: FEED_URL,
      currentVersion: CURRENT_VERSION,
      updatesDirectory,
      fetchImpl,
      events: { onReady: () => undefined, onProblem: () => undefined }
    })

    checker.checkNow()
    await vi.advanceTimersByTimeAsync(10)
    checker.stop()

    return fetched
  }

  it('downloads a newer release and no other', async () => {
    const forSame = await urlsFetchedWhenFeedOffers('1.0.0')
    const forOlder = await urlsFetchedWhenFeedOffers('0.9.0')
    const forNewer = await urlsFetchedWhenFeedOffers('1.0.1')

    expect(forSame).toEqual([FEED_URL])
    expect(forOlder).toEqual([FEED_URL])
    expect(forNewer).toEqual([FEED_URL, ASSET_URL])
  })
})
