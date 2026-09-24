import { describe, expect, it } from 'vitest'

import { isOpenableLink } from './links'

describe('the links a Cmd+click may open', () => {
  it('opens web and mail links', () => {
    expect(isOpenableLink('https://example.com/path?q=1')).toBe(true)
    expect(isOpenableLink('HTTP://example.com')).toBe(true)
    expect(isOpenableLink('mailto:someone@example.com')).toBe(true)
  })

  it('refuses every other scheme, and anything that is not a URL', () => {
    expect(isOpenableLink('file:///etc/passwd')).toBe(false)
    expect(isOpenableLink('javascript:alert(1)')).toBe(false)
    expect(isOpenableLink('x-apple.systempreferences:com.apple.preference')).toBe(false)
    expect(isOpenableLink('example.com')).toBe(false)
    expect(isOpenableLink('')).toBe(false)
  })
})
