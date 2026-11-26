import { expect, it } from 'vitest'

import { afterShowing, viewed, WARM_VIEWS, type Recency } from './shown'

function names(count: number): string[] {
  const ids: string[] = []
  for (let index = 0; index < count; index += 1) {
    ids.push(`t${index}`)
  }
  return ids
}

function showOneAfterAnother(ids: string[]): Recency {
  let recency: Recency = []
  for (const id of ids) {
    recency = afterShowing(recency, [id])
  }
  return recency
}

it('puts what is on screen in front, in its own order, without duplicates', () => {
  const afterGroup = afterShowing(['c', 'b', 'a'], ['x', 'y'])
  expect(afterGroup).toEqual(['x', 'y', 'c', 'b', 'a'])

  const afterAgain = afterShowing(afterGroup, ['b'])
  expect(afterAgain).toEqual(['b', 'x', 'y', 'c', 'a'])
})

it('gives a view to what is on screen and the most recent others, up to the warm count', () => {
  const ids = names(10)
  const open = new Set(ids)
  const recency = showOneAfterAnother(ids)

  const afterLast = viewed(recency, ['t9'], open)
  expect([...afterLast].sort()).toEqual(ids.slice(2).sort())

  const backToFirst = afterShowing(recency, ['t0'])
  const afterOldest = viewed(backToFirst, ['t0'], open)
  expect(afterOldest.has('t0')).toBe(true)
  expect(afterOldest.has('t2')).toBe(false)
  expect(afterOldest.size).toBe(WARM_VIEWS)

  const split = viewed(backToFirst, ['t5', 't6', 't7'], open)
  expect(split.size).toBe(WARM_VIEWS)
  expect(split.has('t5') && split.has('t6') && split.has('t7')).toBe(true)

  const closed = new Set(ids.filter((id) => id !== 't9'))
  expect(viewed(recency, ['t8'], closed).has('t9')).toBe(false)

  const nine = names(9)
  const everyone = viewed(afterShowing(recency, nine), nine, new Set(ids))
  expect([...everyone].sort()).toEqual(nine.sort())
})
