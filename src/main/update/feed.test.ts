import { describe, expect, it } from 'vitest'

import { parseRelease } from './feed'

const DMG = {
  name: 'taco-shells-0.7.0-arm64.dmg',
  size: 128478405,
  browser_download_url:
    'https://github.com/ldelvoye/taco-shells/releases/download/v0.7.0/taco-shells-0.7.0-arm64.dmg'
}
const ZIP = {
  name: 'taco-shells-0.7.0-arm64.zip',
  size: 123456789,
  browser_download_url:
    'https://github.com/ldelvoye/taco-shells/releases/download/v0.7.0/taco-shells-0.7.0-arm64.zip'
}

function release(overrides: Record<string, unknown>): Record<string, unknown> {
  return { tag_name: 'v0.7.0', prerelease: false, assets: [DMG, ZIP], ...overrides }
}

describe('the release the updater will offer', () => {
  it('picks the zip out of a GitHub release', () => {
    expect(parseRelease(release({}))).toEqual({
      version: '0.7.0',
      assetUrl: ZIP.browser_download_url,
      assetSize: ZIP.size
    })
  })

  it('offers nothing for a release it cannot use', () => {
    expect(parseRelease(release({ assets: [DMG] }))).toBeNull()
    expect(parseRelease(release({ prerelease: true }))).toBeNull()
    expect(parseRelease(release({ assets: undefined }))).toBeNull()
    expect(parseRelease(release({ tag_name: 'nightly' }))).toBeNull()
    expect(parseRelease(release({ assets: [{ ...ZIP, size: 0 }] }))).toBeNull()
    expect(parseRelease(release({ assets: [{ ...ZIP, browser_download_url: 7 }] }))).toBeNull()
  })

  it('tolerates any shape without throwing', () => {
    expect(parseRelease(null)).toBeNull()
    expect(parseRelease('release')).toBeNull()
    expect(parseRelease(release({ assets: [null, 3] }))).toBeNull()
  })
})
