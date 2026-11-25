import type { SessionId } from '@protocol/ipc'
import type { AgentRow, AgentRows, AgentSignal, RowState } from '@protocol/rows'

function withRow(rows: AgentRows, pane: SessionId, row: AgentRow): AgentRows {
  const next = new Map(rows)
  next.set(pane, row)
  return next
}

function withoutRow(rows: AgentRows, pane: SessionId): AgentRows {
  const next = new Map(rows)
  next.delete(pane)
  return next
}

function stateAfterSignal(signal: AgentSignal, onScreen: boolean): RowState {
  if (signal === 'stopped') {
    if (onScreen) {
      return 'idle'
    }
    return 'done'
  }
  if (signal === 'needs') {
    return 'needs'
  }
  return 'working'
}

export function rowsAfterSignal(
  rows: AgentRows,
  pane: SessionId,
  harness: string,
  sessionId: string,
  signal: AgentSignal,
  onScreen: boolean
): AgentRows {
  const row = rows.get(pane)
  const isCurrent = row !== undefined && row.harness === harness && row.sessionId === sessionId
  if (signal === 'start' || signal === 'resume') {
    if (isCurrent) {
      return rows
    }
    const hasConversation = signal === 'resume'
    return withRow(rows, pane, { harness, sessionId, state: 'idle', hasConversation })
  }
  if (row === undefined || !isCurrent) {
    return rows
  }
  if (signal === 'end') {
    return withoutRow(rows, pane)
  }
  // Every signal left comes from a turn, and a turn begins with a prompt.
  const state = stateAfterSignal(signal, onScreen)
  if (row.state === state && row.hasConversation) {
    return rows
  }
  return withRow(rows, pane, { harness, sessionId, state, hasConversation: true })
}

export function rowsAfterScreen(rows: AgentRows, onScreen: readonly SessionId[]): AgentRows {
  let next = rows
  for (const pane of onScreen) {
    const row = rows.get(pane)
    if (row !== undefined && row.state === 'done') {
      next = withRow(next, pane, { ...row, state: 'idle' })
    }
  }
  return next
}

// Stop does not fire when a turn is interrupted, so an idle title is the fallback that ends working and needs.
export function rowsAfterIdleTitle(rows: AgentRows, pane: SessionId): AgentRows {
  const row = rows.get(pane)
  if (row === undefined) {
    return rows
  }
  if (row.state === 'working' || row.state === 'needs') {
    return withRow(rows, pane, { ...row, state: 'idle' })
  }
  return rows
}

export function rowsAfterExit(rows: AgentRows, pane: SessionId): AgentRows {
  if (!rows.has(pane)) {
    return rows
  }
  return withoutRow(rows, pane)
}
