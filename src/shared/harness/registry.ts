import { claude } from './claude'
import type { Harness } from './types'

export const HARNESSES: readonly Harness[] = [claude]
export const DEFAULT_HARNESS: Harness = claude

export function harnessOf(id: unknown, harnesses: readonly Harness[] = HARNESSES): Harness | null {
  const found = harnesses.find((harness) => harness.id === id)
  if (found === undefined) {
    return null
  }
  return found
}
