import type { SessionId } from '@protocol/ipc'
import type { AgentRows, RowState } from '@protocol/rows'

/** The one state a collapsed group shows for the sessions inside it, or null when none is worth a marker. */
export function summaryState(rows: AgentRows, panes: readonly SessionId[]): RowState | null {
  const states: RowState[] = []
  for (const pane of panes) {
    const row = rows.get(pane)
    if (row !== undefined) {
      states.push(row.state)
    }
  }
  if (states.includes('needs')) {
    return 'needs'
  }
  if (states.includes('working')) {
    return 'working'
  }
  if (states.includes('done')) {
    return 'done'
  }
  return null
}

export function waitingCount(rows: AgentRows): number {
  let count = 0
  for (const row of rows.values()) {
    if (row.state === 'needs') {
      count += 1
    }
  }
  return count
}
