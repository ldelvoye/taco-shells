import type { SessionId } from './ipc'

export type RowState = 'working' | 'needs' | 'done' | 'idle'
// 'resume' is a start whose session already holds a conversation.
export type AgentSignal = 'start' | 'resume' | 'working' | 'needs' | 'stopped' | 'end'

export interface AgentRow {
  harness: string
  sessionId: string
  state: RowState
  hasConversation: boolean
}

export type AgentRows = ReadonlyMap<SessionId, AgentRow>

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
