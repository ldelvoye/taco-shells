import type { RowColor } from '@shared/model'

export function rowColorVariable(color: RowColor): string {
  return `var(--row-color-${color})`
}
