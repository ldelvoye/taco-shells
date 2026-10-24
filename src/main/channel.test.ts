import { afterEach, describe, expect, it, vi } from 'vitest'

// The module reads the baked-in channel when it loads, so each case has to set
// the global and import a fresh copy.
async function buildFor(channel: string) {
  vi.stubGlobal('__CHANNEL__', channel)
  vi.resetModules()
  return import('./channel')
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('the two builds stay apart', () => {
  it('gives each channel its own name and config directory', async () => {
    const stable = await buildFor('stable')
    expect(stable.isStable).toBe(true)
    expect(stable.applicationName).toBe('Taco Shells')
    expect(stable.configDirectoryName).toBe('.taco-shells')

    const development = await buildFor('development')
    expect(development.isStable).toBe(false)
    expect(development.applicationName).toBe('Taco Shells Dev')
    expect(development.configDirectoryName).toBe('.taco-shells-dev')
  })

  // A build that lost its channel must not read the config the app in daily use
  // depends on, so anything unrecognised has to fall to development.
  it('treats anything but stable as development', async () => {
    const unrecognised = ['', 'Stable', 'production', 'undefined']

    for (const channel of unrecognised) {
      const build = await buildFor(channel)
      expect(build.isStable).toBe(false)
      expect(build.configDirectoryName).toBe('.taco-shells-dev')
      expect(build.applicationName).toBe('Taco Shells Dev')
    }
  })
})
