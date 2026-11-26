import { expect, it } from 'vitest'

import { holdsColorQuery, leaveAnswersToTheRuntime } from './stream'

it('tells a sequence that asks for a color from one that sets it', () => {
  const cases: [4 | 10 | 11 | 12, string, boolean][] = [
    [11, '?', true],
    [11, '#ffffff', false],
    [10, '#ffffff;?', true],
    [12, 'rgb:00/00/00', false],
    [4, '1;?', true],
    [4, '1;#ffffff', false],
    [4, '1;#ffffff;9;?', true],
    [4, '?;#ffffff', false]
  ]

  for (const [sequence, data, asks] of cases) {
    expect(holdsColorQuery(sequence, data), `OSC ${sequence};${data}`).toBe(asks)
  }
})

it('takes a color query away from a view for each of the four color sequences, and leaves a color being set', () => {
  const handlers = new Map<number, (data: string) => boolean>()
  leaveAnswersToTheRuntime({
    parser: {
      registerCsiHandler: () => undefined,
      registerDcsHandler: () => undefined,
      registerOscHandler: (ident, callback) => {
        handlers.set(ident, callback)
      }
    }
  })

  const idents = [...handlers.keys()]
  expect(idents.sort((first, second) => first - second)).toEqual([4, 10, 11, 12])
  for (const ident of [10, 11, 12]) {
    const handler = handlers.get(ident)
    expect(handler?.('?'), `OSC ${ident} asking`).toBe(true)
    expect(handler?.('#ffffff'), `OSC ${ident} setting`).toBe(false)
  }
  expect(handlers.get(4)?.('1;?')).toBe(true)
  expect(handlers.get(4)?.('1;#ffffff')).toBe(false)
})
