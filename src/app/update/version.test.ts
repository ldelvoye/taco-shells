import { describe, expect, it } from 'vitest'

import { isNewerVersion, isVersion } from './version'

describe('which versions may trigger an update', () => {
  it('compares numerically, and only for plain releases', () => {
    expect(isNewerVersion('0.7.0', '0.6.0')).toBe(true)
    expect(isNewerVersion('0.6.0', '0.6.0')).toBe(false)
    expect(isNewerVersion('0.10.0', '0.9.0')).toBe(true)
    expect(isNewerVersion('0.0.0+v0.7.1-3-gabc', '0.6.0')).toBe(false)
    expect(isNewerVersion('0.7.0', '0.0.0+v0.7.1-3-gabc')).toBe(false)
  })

  it('accepts only three dotted numbers', () => {
    expect(isVersion('1.22.3')).toBe(true)
    expect(isVersion('v1.2.3')).toBe(false)
    expect(isVersion('1.2')).toBe(false)
  })
})
