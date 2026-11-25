import { expect, test } from 'vitest'
import { summarize } from './stats.mts'

test('summarize reports nearest-rank percentiles and does not reorder its input', () => {
  const samples = [9, 1, 5, 3, 7, 2, 8, 4, 6, 10]
  expect(summarize(samples)).toEqual({ count: 10, p50: 5, p95: 10, max: 10 })
  expect(samples[0]).toBe(9)
  expect(summarize([4])).toEqual({ count: 1, p50: 4, p95: 4, max: 4 })
  expect(summarize([])).toEqual({ count: 0, p50: 0, p95: 0, max: 0 })
})
