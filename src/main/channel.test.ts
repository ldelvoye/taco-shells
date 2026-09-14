import { describe, expect, it } from 'vitest'

import { STABLE, applicationName, configDirectoryName } from './channel'

describe('the two builds stay apart', () => {
  it('gives each channel its own name and config directory', () => {
    expect(applicationName(STABLE)).toBe('Taco Shells')
    expect(configDirectoryName(STABLE)).toBe('.taco-shells')

    expect(applicationName('development')).toBe('Taco Shells Dev')
    expect(configDirectoryName('development')).toBe('.taco-shells-dev')
  })

  // A build that lost its channel must not read the config the app in daily use
  // depends on, so anything unrecognised has to fall to development.
  it('treats anything but stable as development', () => {
    const unrecognised = ['', 'Stable', 'production', 'undefined']

    for (const channel of unrecognised) {
      expect(configDirectoryName(channel)).toBe('.taco-shells-dev')
      expect(applicationName(channel)).toBe('Taco Shells Dev')
    }
  })
})
