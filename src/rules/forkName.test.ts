import { describe, expect, it } from 'vitest'
import { forkNameFor } from './forkName'

describe('forkNameFor', () => {
  it('marks the first fork with the glyph and numbers the ones after it', () => {
    expect(forkNameFor('Tacos fact', ['Tacos fact'])).toBe('⑂ Tacos fact')
    expect(forkNameFor('Tacos fact', ['Tacos fact', '⑂ Tacos fact'])).toBe('⑂2 Tacos fact')
    // a fork of a fork continues the count
    expect(forkNameFor('⑂ Tacos fact', ['Tacos fact', '⑂ Tacos fact'])).toBe('⑂2 Tacos fact')
    expect(forkNameFor('⑂2 Tacos fact', ['⑂2 Tacos fact'])).toBe('⑂3 Tacos fact')
    // one more than the highest open, not the first gap
    expect(forkNameFor('Tacos fact', ['Tacos fact', '⑂3 Tacos fact'])).toBe('⑂4 Tacos fact')
    // forks of another conversation do not count
    expect(forkNameFor('Tacos fact', ['⑂2 Burrito fact'])).toBe('⑂ Tacos fact')
    expect(forkNameFor('', [])).toBe('⑂')
  })
})
