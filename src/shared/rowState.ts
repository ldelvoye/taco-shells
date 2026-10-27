import type { SessionId } from './ipc'

const NEEDS_TOOLS: ReadonlySet<string> = new Set(['AskUserQuestion', 'ExitPlanMode'])
const NEEDS_NOTIFICATIONS: ReadonlySet<string> = new Set([
  'permission_prompt',
  'elicitation_dialog',
  'elicitation_url_dialog',
  'agent_needs_input'
])

export type RowState = 'working' | 'needs' | 'done' | 'idle'
export type ClaudeSignal = 'start' | 'working' | 'needs' | 'stopped' | 'end'

export interface ClaudeRow {
  claudeSessionId: string
  state: RowState
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
  if (signal === 'start') {
    if (row !== undefined && row.claudeSessionId === claudeSessionId) {
      return rows
    }
    return withRow(rows, pane, { claudeSessionId, state: 'idle' })
  }
  if (row === undefined || row.claudeSessionId !== claudeSessionId) {
    return rows
  }
  if (signal === 'end') {
    return withoutRow(rows, pane)
  }
  const state = stateAfterSignal(signal, onScreen)
  if (row.state === state) {
    return rows
  }
  return withRow(rows, pane, { claudeSessionId, state })
}

export function rowsAfterScreen(rows: ClaudeRows, onScreen: readonly SessionId[]): ClaudeRows {
  let next = rows
  for (const pane of onScreen) {
    const row = rows.get(pane)
    if (row !== undefined && row.state === 'done') {
      next = withRow(next, pane, { claudeSessionId: row.claudeSessionId, state: 'idle' })
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
    return withRow(rows, pane, { claudeSessionId: row.claudeSessionId, state: 'idle' })
  }
  return rows
}

export function rowsAfterExit(rows: ClaudeRows, pane: SessionId): ClaudeRows {
  if (!rows.has(pane)) {
    return rows
  }
  return withoutRow(rows, pane)
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
