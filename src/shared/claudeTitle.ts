// Claude Code prefixes its terminal title with a status glyph; ✳ means idle, anything else means working.
const IDLE_GLYPH = '✳'
const LEADING_GLYPH = /^([^\p{L}\p{N}\s])\p{M}? /u

export interface ClaudeTitle {
  name: string
  idle: boolean
}

export function claudeTitleOf(title: string): ClaudeTitle {
  const match = LEADING_GLYPH.exec(title)
  if (match === null) {
    return { name: title, idle: false }
  }
  const glyph = match[1]
  const name = title.slice(match[0].length)
  return { name, idle: glyph === IDLE_GLYPH }
}
