import { expect, it } from 'vitest'

import { trackColorSets } from './colorSets'

function set(ident: string, color: string): string {
  return `\x1b]${ident};${color}\x1b\\`
}

it('keeps the colors a program set, the last one for each, and nothing that is not a color', () => {
  const colors = trackColorSets()
  const notAColor = 'x'.repeat(1000)

  colors.set(4, '1;#aa0000;9;rgb:ff/00/00')
  colors.set(4, '1;#bb0000')
  colors.set(4, '2;?')
  colors.set(4, `5;${notAColor}`)
  colors.set(4, '6;#12345')
  colors.set(11, 'red')
  colors.set(4, '300;#ffffff')
  colors.set(10, '#111111;#222222')
  colors.set(12, '?')
  colors.set(12, '#333333;#444444')

  expect(colors.sequences()).toBe(
    set('4;1', '#bb0000') +
      set('4;9', 'rgb:ff/00/00') +
      set('10', '#111111') +
      set('11', '#222222') +
      set('12', '#333333')
  )
})

it('forgets a color the program reset', () => {
  const colors = trackColorSets()
  colors.set(4, '1;#aa0000;2;#00aa00;3;#0000aa')
  colors.set(10, '#111111;#222222;#333333')

  colors.reset(104, '2')
  colors.reset(111, '')
  expect(colors.sequences()).toBe(
    set('4;1', '#aa0000') + set('4;3', '#0000aa') + set('10', '#111111') + set('12', '#333333')
  )

  colors.reset(104, '')
  colors.reset(110, '')
  colors.reset(112, '')
  expect(colors.sequences()).toBe('')
})
