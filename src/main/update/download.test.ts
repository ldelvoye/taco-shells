import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { downloadAndExtract, extractedAppIn, removeOtherVersions } from './download'
import type { ReleaseInfo } from './feed'

const VERSION = '0.6.0'

let scratch: string
let zipBytes: Buffer

function releaseWithSize(assetSize: number): ReleaseInfo {
  return { version: VERSION, assetUrl: 'https://example.com/taco-shells.zip', assetSize }
}

function respondWith(body: Buffer): typeof fetch {
  return async () => new Response(body)
}

beforeAll(() => {
  scratch = mkdtempSync(join(tmpdir(), 'taco-update-'))
  const executableDirectory = join(scratch, 'source', 'Fake.app', 'Contents', 'MacOS')
  mkdirSync(executableDirectory, { recursive: true })
  writeFileSync(join(executableDirectory, 'Fake'), '#!/bin/sh\n')
  const sourceApp = join(scratch, 'source', 'Fake.app')
  const zipPath = join(scratch, 'fake.zip')
  execFileSync('ditto', ['-c', '-k', '--keepParent', sourceApp, zipPath])
  zipBytes = readFileSync(zipPath)
})

afterAll(() => {
  rmSync(scratch, { recursive: true, force: true })
})

describe('downloading an update', () => {
  it('extracts the app when the byte count matches the release', async () => {
    const updates = join(scratch, 'updates-ok')
    const release = releaseWithSize(zipBytes.length)

    const app = await downloadAndExtract(release, updates, respondWith(zipBytes))

    expect(app).not.toBeNull()
    expect(app?.endsWith('Fake.app')).toBe(true)
    expect(existsSync(join(app as string, 'Contents', 'MacOS', 'Fake'))).toBe(true)
  })

  it('removes a truncated download and offers nothing', async () => {
    const updates = join(scratch, 'updates-short')
    const release = releaseWithSize(zipBytes.length - 1)

    const app = await downloadAndExtract(release, updates, respondWith(zipBytes))

    expect(app).toBeNull()
    expect(existsSync(join(updates, VERSION))).toBe(false)
    expect(existsSync(join(updates, `${VERSION}.partial`))).toBe(false)
  })

  it('offers nothing when the fetch throws or the server answers an error', async () => {
    const release = releaseWithSize(zipBytes.length)
    const throwing: typeof fetch = async () => {
      throw new Error('offline')
    }
    const notFound: typeof fetch = async () => new Response('nope', { status: 404 })

    const afterThrow = await downloadAndExtract(release, join(scratch, 'updates-throw'), throwing)
    const afterNotFound = await downloadAndExtract(release, join(scratch, 'updates-404'), notFound)

    expect(afterThrow).toBeNull()
    expect(afterNotFound).toBeNull()
  })
})

describe('the extracted app', () => {
  it('is null when a directory holds more than one app', () => {
    const directory = join(scratch, 'two-apps')
    mkdirSync(join(directory, 'One.app'), { recursive: true })
    mkdirSync(join(directory, 'Two.app'), { recursive: true })

    expect(extractedAppIn(directory)).toBeNull()
  })
})

describe('removing old downloads', () => {
  it('keeps only the named version, and tolerates a missing directory', () => {
    const updates = join(scratch, 'updates-prune')
    mkdirSync(join(updates, '0.5.0'), { recursive: true })
    mkdirSync(join(updates, '0.6.0'), { recursive: true })

    removeOtherVersions(updates, '0.6.0')

    expect(existsSync(join(updates, '0.5.0'))).toBe(false)
    expect(existsSync(join(updates, '0.6.0'))).toBe(true)
    expect(() => removeOtherVersions(join(scratch, 'missing'), null)).not.toThrow()
  })
})
