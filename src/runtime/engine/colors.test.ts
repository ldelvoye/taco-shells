import { describe, expect, it } from 'vitest'

import type { TerminalColors } from '@protocol/theme'

import { colorAnswers } from './colors'

const COLORS: TerminalColors = {
  background: '#102030',
  foreground: '#a0b0c0',
  cursor: '#ff8000',
  cursorAccent: '#000001',
  selectionBackground: '#000002',
  black: '#010101',
  red: '#aa0102',
  green: '#03aa04',
  yellow: '#050606',
  blue: '#070708',
  magenta: '#090a0a',
  cyan: '#0b0c0c',
  white: '#0d0e0e',
  brightBlack: '#0f1010',
  brightRed: '#ee1112',
  brightGreen: '#131414',
  brightYellow: '#151616',
  brightBlue: '#171818',
  brightMagenta: '#191a1a',
  brightCyan: '#1b1c1c',
  brightWhite: '#1d1e1e'
}

function reply(ident: string, rgb: string): string {
  return `\x1b]${ident};rgb:${rgb}\x1b\\`
}

describe('colorAnswers', () => {
  it('answers each query in a sequence from the colors it was given, and nothing else', () => {
    expect(colorAnswers(11, '?', COLORS)).toEqual([reply('11', '1010/2020/3030')])
    expect(colorAnswers(10, '?;?', COLORS)).toEqual([
      reply('10', 'a0a0/b0b0/c0c0'),
      reply('11', '1010/2020/3030')
    ])
    expect(colorAnswers(12, '?;?', COLORS)).toEqual([reply('12', 'ffff/8080/0000')])
    expect(colorAnswers(10, '#ffffff', COLORS)).toEqual([])
    expect(colorAnswers(4, '1;?;9;?', COLORS)).toEqual([
      reply('4;1', 'aaaa/0101/0202'),
      reply('4;9', 'eeee/1111/1212')
    ])
    expect(colorAnswers(4, '16;?', COLORS)).toEqual([reply('4;16', '0000/0000/0000')])
    expect(colorAnswers(4, '231;?', COLORS)).toEqual([reply('4;231', 'ffff/ffff/ffff')])
    expect(colorAnswers(4, '232;?', COLORS)).toEqual([reply('4;232', '0808/0808/0808')])
    expect(colorAnswers(4, '255;?', COLORS)).toEqual([reply('4;255', 'eeee/eeee/eeee')])
    expect(colorAnswers(4, '256;?', COLORS)).toEqual([])
  })

  it('answers nothing for what is not quite a query', () => {
    expect(colorAnswers(11, '?x', COLORS)).toEqual([])
    expect(colorAnswers(4, 'x;?', COLORS)).toEqual([])
    expect(colorAnswers(4, '1;2;?', COLORS)).toEqual([])
  })
})
