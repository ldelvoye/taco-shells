const FORK_GLYPH = '⑂'
const FORK_MARKER = new RegExp(`^${FORK_GLYPH}(\\d*)(?: |$)`)

interface ForkMark {
  number: number
  base: string
}

// A bare glyph is fork 1, and a name with no marker is fork 0: the original.
function forkMarkOf(name: string): ForkMark {
  const match = FORK_MARKER.exec(name)
  if (match === null) {
    return { number: 0, base: name }
  }

  const base = name.slice(match[0].length)
  const digits = match[1]
  if (digits === '') {
    return { number: 1, base }
  }

  const number = Number(digits)
  return { number, base }
}

/** The name for a fork of a row, numbered past open forks: "Tacos fact" gives "⑂ Tacos fact". */
export function forkNameFor(sourceName: string, openNames: readonly string[]): string {
  const source = forkMarkOf(sourceName)

  let highest = 0
  for (const openName of openNames) {
    const open = forkMarkOf(openName)
    if (open.base !== source.base) {
      continue
    }
    if (open.number > highest) {
      highest = open.number
    }
  }

  const next = highest + 1
  let marker = FORK_GLYPH
  if (next > 1) {
    marker = `${FORK_GLYPH}${next}`
  }

  if (source.base === '') {
    return marker
  }
  return `${marker} ${source.base}`
}
