import type { SessionId } from './ipc'

const NEEDS_TOOLS: ReadonlySet<string> = new Set(['AskUserQuestion', 'ExitPlanMode'])
const NEEDS_NOTIFICATIONS: ReadonlySet<string> = new Set([
  'permission_prompt',
  'elicitation_dialog',
  'elicitation_url_dialog',
  'agent_needs_input'
])

export type RowState = 'working' | 'needs' | 'done' | 'idle'
// 'resume' is a start whose session already holds a conversation.
export type ClaudeSignal = 'start' | 'resume' | 'working' | 'needs' | 'stopped' | 'end'

export interface ClaudeRow {
  claudeSessionId: string
  state: RowState
  hasConversation: boolean
}

export type ClaudeRows = ReadonlyMap<SessionId, ClaudeRow>

export function signalForHook(event: string, detail: string | null): ClaudeSignal | null {
  if (event === 'UserPromptSubmit' || event === 'PostToolUse') {
    return 'working'
  }
  if (event === 'PreToolUse') {
    if (detail !== null && NEEDS_TOOLS.has(detail)) {
      return 'needs'
    }
    return 'working'
  }
  if (event === 'Notification') {
    if (detail !== null && NEEDS_NOTIFICATIONS.has(detail)) {
      return 'needs'
    }
    return null
  }
  if (event === 'Stop' || event === 'StopFailure') {
    return 'stopped'
  }
  return null
}

function withRow(rows: ClaudeRows, pane: SessionId, row: ClaudeRow): ClaudeRows {
  const next = new Map(rows)
  next.set(pane, row)
  return next
}

function withoutRow(rows: ClaudeRows, pane: SessionId): ClaudeRows {
  const next = new Map(rows)
  next.delete(pane)
  return next
}

function stateAfterSignal(signal: ClaudeSignal, onScreen: boolean): RowState {
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
  rows: ClaudeRows,
  pane: SessionId,
  claudeSessionId: string,
  signal: ClaudeSignal,
  onScreen: boolean
): ClaudeRows {
  const row = rows.get(pane)
  if (signal === 'start' || signal === 'resume') {
    if (row !== undefined && row.claudeSessionId === claudeSessionId) {
      return rows
    }
    const hasConversation = signal === 'resume'
    return withRow(rows, pane, { claudeSessionId, state: 'idle', hasConversation })
  }
  if (row === undefined || row.claudeSessionId !== claudeSessionId) {
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
  return withRow(rows, pane, { claudeSessionId, state, hasConversation: true })
}

export function rowsAfterScreen(rows: ClaudeRows, onScreen: readonly SessionId[]): ClaudeRows {
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
export function rowsAfterIdleTitle(rows: ClaudeRows, pane: SessionId): ClaudeRows {
  const row = rows.get(pane)
  if (row === undefined) {
    return rows
  }
  if (row.state === 'working' || row.state === 'needs') {
    return withRow(rows, pane, { ...row, state: 'idle' })
  }
  return rows
}

export function rowsAfterExit(rows: ClaudeRows, pane: SessionId): ClaudeRows {
  if (!rows.has(pane)) {
    return rows
  }
  return withoutRow(rows, pane)
}

/** The one state a collapsed group shows for the sessions inside it, or null when none is worth a marker. */
export function summaryState(rows: ClaudeRows, panes: readonly SessionId[]): RowState | null {
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

export function waitingCount(rows: ClaudeRows): number {
  let count = 0
  for (const row of rows.values()) {
    if (row.state === 'needs') {
      count += 1
    }
  }
  return count
}
