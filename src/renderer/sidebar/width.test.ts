import { describe, expect, it } from 'vitest'

import {
  DEFAULT_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  parseRemembered,
  type RememberedWidths,
  widthForScreen,
  widthWithin
} from './width'

describe('widthForScreen', () => {
  it('falls back to the default, an exact match, the nearest key, or the smaller of a tie', () => {
    expect(widthForScreen({}, 1512)).toBe(DEFAULT_SIDEBAR_WIDTH)

    const withExactMatch: RememberedWidths = { '1512': 260 }
    expect(widthForScreen(withExactMatch, 1512)).toBe(260)

    const withTwoKeys: RememberedWidths = { '1512': 260, '3440': 320 }
    expect(widthForScreen(withTwoKeys, 3000)).toBe(320)

    const withATie: RememberedWidths = { '1000': 200, '2000': 300 }
    expect(widthForScreen(withATie, 1500)).toBe(200)
  })
})

describe('widthWithin', () => {
  it('clamps to the floor, to the ceiling, leaves an in-range width alone, and lets a low ceiling win', () => {
    expect(widthWithin(80, 2000)).toBe(MIN_SIDEBAR_WIDTH)
    expect(widthWithin(900, 1200)).toBe(600)
    expect(widthWithin(300, 1200)).toBe(300)
    expect(widthWithin(150, 200)).toBe(100)
  })
})

describe('parseRemembered', () => {
  it('never throws, and drops anything that is not a clean width entry', () => {
    expect(parseRemembered(null)).toEqual({})
    expect(parseRemembered('{not json')).toEqual({})
    expect(parseRemembered('[1512, 3440]')).toEqual({})

    const withJunk = JSON.stringify({
      '1512': 260,
      '3440': 320,
      'not-a-number': 300,
      '-100': 200,
      '0': 200,
      '640': -10,
      '800': null,
      '900': 'wide'
    })
    expect(parseRemembered(withJunk)).toEqual({ '1512': 260, '3440': 320 })
  })
})
