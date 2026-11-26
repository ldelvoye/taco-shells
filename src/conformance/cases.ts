export interface Case {
  name: string
  before: string
  after: string
  known?: string
}

function numberedLines(count: number): string {
  const lines: string[] = []
  for (let n = 1; n <= count; n += 1) {
    lines.push(`line ${n}`)
  }
  return lines.join('\r\n')
}

export const CASES: Case[] = [
  { name: 'plain text with scrollback', before: numberedLines(60), after: '\r\nline 61' },
  { name: 'colors', before: '\x1b[31;44mred\x1b[0m \x1b[1;38;5;200mbold\x1b[0m', after: ' more' },
  { name: 'the alternate screen', before: 'main\r\n\x1b[?1049hALT', after: 'more' },
  { name: 'bracketed paste', before: '\x1b[?2004h', after: '' },
  { name: 'application cursor keys', before: '\x1b[?1h', after: '' },
  { name: 'mouse tracking', before: '\x1b[?1000h', after: '' },
  { name: 'insert mode', before: '\x1b[4h', after: 'abc\x1b[1;1HZ' },
  { name: 'wraparound off', before: '\x1b[?7l', after: 'x'.repeat(100) },
  { name: 'a line filled to the last column', before: 'x'.repeat(80), after: 'y' },
  { name: 'wide characters', before: '日本語 😀 ', after: 'text' },
  { name: 'the hidden cursor', before: '\x1b[?25l', after: '' },
  {
    name: 'scroll region',
    before: '\x1b[5;10r',
    after: '\x1b[10;1H\nA\nB\nC',
    known: 'the scroll region is not serialized'
  },
  {
    name: 'origin mode',
    before: '\x1b[5;10r\x1b[?6h',
    after: '\x1b[1;1HX',
    known: 'origin mode is not serialized'
  },
  {
    name: 'saved cursor',
    before: '\x1b[7;7H\x1b7\x1b[1;1H',
    after: '\x1b8Y',
    known: 'the saved cursor is not serialized'
  },
  {
    name: 'the line-drawing character set',
    before: '\x1b(0',
    after: 'qqq',
    known: 'the character set is not serialized'
  },
  {
    name: 'tab stops',
    before: '\x1b[3g\x1b[1;5H\x1bH\x1b[1;1H',
    after: '\tT',
    known: 'tab stops are not serialized'
  }
]
