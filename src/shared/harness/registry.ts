import { claude } from './claude'
import type { AgentTitle, Harness } from './types'

export const HARNESSES: readonly Harness[] = [claude]
export const DEFAULT_HARNESS: Harness = claude
// Saved files from before harnesses name no harness: they mean Claude.
export const LEGACY_HARNESS: Harness = claude

export function harnessOf(id: unknown, harnesses: readonly Harness[] = HARNESSES): Harness | null {
  const found = harnesses.find((harness) => harness.id === id)
  if (found === undefined) {
    return null
  }
  return found
}

/** A terminal title as a row name, read by the row's harness; the raw title when the harness reads no titles. */
export function agentTitleOf(
  harnessId: string,
  title: string,
  harnesses: readonly Harness[] = HARNESSES
): AgentTitle {
  const harness = harnessOf(harnessId, harnesses)
  if (harness === null || harness.titleOf === undefined) {
    return { name: title, idle: false }
  }
  return harness.titleOf(title)
}
