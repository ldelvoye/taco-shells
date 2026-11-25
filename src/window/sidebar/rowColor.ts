import type { RowColor } from '@protocol/arrangement'

export function rowColorVariable(color: RowColor): string {
  return `var(--row-color-${color})`
}
