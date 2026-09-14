import { describe, expect, it } from 'vitest'

import { applyBump, bumpFor, nextVersion } from './version.mjs'

describe('bumpFor', () => {
  it('reads the bump out of conventional commit subjects', () => {
    expect(bumpFor(['feat: split terminals into panes'])).toBe('minor')
    expect(bumpFor(['fix(pty): let shells exit with the process'])).toBe('patch')
    expect(bumpFor(['chore: bump dependencies'])).toBe('patch')
    expect(bumpFor(['feat(config)!: drop the old keybinding format'])).toBe('major')
  })

  it('takes the largest bump any one commit calls for', () => {
    const messages = ['fix: a cleanup round', 'feat: the feature itself', 'docs: readme']

    expect(bumpFor(messages)).toBe('minor')
  })

  it('finds a breaking change declared in a commit body', () => {
    const message = 'feat: rework config\n\nBREAKING CHANGE: settings.json moved'

    expect(bumpFor([message])).toBe('major')
  })
})

describe('applyBump', () => {
  it('counts numbers up rather than appending to the string', () => {
    expect(applyBump('1.2.9', 'patch')).toBe('1.2.10')
    expect(applyBump('1.9.0', 'minor')).toBe('1.10.0')
  })

  it('resets the numbers below the one it moves', () => {
    expect(applyBump('1.2.3', 'minor')).toBe('1.3.0')
    expect(applyBump('1.2.3', 'major')).toBe('2.0.0')
  })

  it('keeps a breaking change below 1.0.0 on the minor', () => {
    expect(applyBump('0.2.3', 'major')).toBe('0.3.0')
  })
})

describe('nextVersion', () => {
  it('walks a feature and two cleanup rounds the way the history reads', () => {
    const afterFeature = nextVersion('0.1.0', ['feat: a new thing'])
    const afterFirstCleanup = nextVersion(afterFeature, ['fix: tidy the new thing'])
    const afterSecondCleanup = nextVersion(afterFirstCleanup, ['fix: tidy it again'])

    expect(afterFeature).toBe('0.2.0')
    expect(afterFirstCleanup).toBe('0.2.1')
    expect(afterSecondCleanup).toBe('0.2.2')
  })
})
