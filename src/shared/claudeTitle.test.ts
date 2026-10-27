import { expect, it } from 'vitest'

import { claudeTitleOf } from './claudeTitle'

it('strips the leading glyph and reports whether it was the idle one', () => {
  expect(claudeTitleOf('✳ INFRENG-813')).toEqual({ name: 'INFRENG-813', idle: true })
  expect(claudeTitleOf('◑ INFRENG-823')).toEqual({ name: 'INFRENG-823', idle: false })
  expect(claudeTitleOf('INFRENG-714')).toEqual({ name: 'INFRENG-714', idle: false })
  expect(claudeTitleOf('✳ ')).toEqual({ name: '', idle: true })
})
