import { expect, it } from 'vitest'

import { holdsColorQuery } from './stream'

it('tells a sequence that asks for a color from one that sets it', () => {
  const cases: [4 | 10 | 11 | 12, string, boolean][] = [
    [11, '?', true],
    [11, '#ffffff', false],
    [10, '#ffffff;?', true],
    [12, 'rgb:00/00/00', false],
    [4, '1;?', true],
    [4, '1;#ffffff', false],
    [4, '1;#ffffff;9;?', true]
  ]

  for (const [sequence, data, asks] of cases) {
    expect(holdsColorQuery(sequence, data), `OSC ${sequence};${data}`).toBe(asks)
  }
})
