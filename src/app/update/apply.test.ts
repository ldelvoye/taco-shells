import { describe, expect, it } from 'vitest'

import { installedBundlePath } from './apply'

describe('the bundle an update may replace', () => {
  it('is the app under /Applications, and nothing else', () => {
    const installed = installedBundlePath('/Applications/Taco Shells.app/Contents/MacOS/Taco Shells')
    expect(installed).toBe('/Applications/Taco Shells.app')

    const devBuild = installedBundlePath(
      '/Users/x/code/taco-shells/dist.noindex/mac-arm64/Taco Shells Dev.app/Contents/MacOS/Taco Shells Dev'
    )
    expect(devBuild).toBeNull()

    const notAnExecutable = installedBundlePath('/Applications/Taco Shells.app')
    expect(notAnExecutable).toBeNull()
  })
})
