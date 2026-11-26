import type { SessionId } from '@protocol/ipc'

// A page gets about sixteen graphics contexts, and each view takes one.
export const WARM_VIEWS = 8

/** Terminals in the order they were last on screen, most recent first. */
export type Recency = readonly SessionId[]

export function afterShowing(recency: Recency, onScreen: readonly SessionId[]): Recency {
  const shown = new Set(onScreen)
  const others = recency.filter((id) => !shown.has(id))
  return [...shown, ...others]
}

export function viewed(
  recency: Recency,
  onScreen: readonly SessionId[],
  open: ReadonlySet<SessionId>
): Set<SessionId> {
  const result = new Set<SessionId>()
  for (const id of onScreen) {
    if (open.has(id)) {
      result.add(id)
    }
  }

  for (const id of recency) {
    if (result.size >= WARM_VIEWS) {
      break
    }
    if (open.has(id)) {
      result.add(id)
    }
  }
  return result
}
