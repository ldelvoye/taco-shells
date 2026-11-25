export interface Summary {
  count: number
  p50: number
  p95: number
  max: number
}

function nearestRank(ascending: number[], percentile: number): number {
  const position = Math.ceil((percentile / 100) * ascending.length)
  return ascending[position - 1]
}

export function summarize(samples: number[]): Summary {
  if (samples.length === 0) {
    return { count: 0, p50: 0, p95: 0, max: 0 }
  }
  const ascending = [...samples].sort((a, b) => a - b)
  const p50 = nearestRank(ascending, 50)
  const p95 = nearestRank(ascending, 95)
  const max = ascending[ascending.length - 1]
  return { count: ascending.length, p50, p95, max }
}
