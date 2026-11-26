import { agentTitleOf } from '@agents/registry'
import type {
  Dragged,
  Drop,
  Group,
  GroupId,
  RowColor,
  TabGroupId,
  Workspace
} from '@protocol/arrangement'
import type { AgentSignalEvent, SessionId } from '@protocol/ipc'
import type { AgentRows } from '@protocol/rows'
import { activeSessionOf, emptyWorkspace } from '@rules/arrangement'
import { activatePane } from '@rules/focus'
import { applyDrop } from '@rules/transitions/applyDrop'
import {
  addGroup,
  addGroupTo,
  closePane,
  forkPane,
  moveDivider,
  renamePane,
  setPaneColor,
  splitPane,
  toggleActivePaneColor
} from '@rules/transitions/arrangement'
import {
  createTabGroup,
  recolorTabGroup,
  renameTabGroup,
  toggleTabGroup,
  ungroupTabGroup
} from '@rules/transitions/tabGroups'
import { waitingAfterAttention, waitingAfterExit, waitingAfterFocus } from './attention'
import { rowsAfterExit, rowsAfterIdleTitle, rowsAfterScreen, rowsAfterSignal } from './rowState'

export interface WorkspaceState {
  // Null is the app before its first terminal exists, which is not the same
  // state as a workspace whose last terminal has just closed.
  workspace: Workspace | null
  agentRows: AgentRows
  waiting: SessionId | null
  rawTitles: ReadonlyMap<SessionId, string>
  // Bumped whenever the keyboard has to go back to the active terminal: a drop, a collapse, an app activation, a hook asking for a terminal.
  refocus: number
}

export type WorkspaceEvent =
  | { type: 'terminalOpened'; group: Group }
  | { type: 'terminalOpenedIn'; group: Group; tabGroup: TabGroupId | null }
  | { type: 'workspaceRestored'; workspace: Workspace }
  | { type: 'splitSpawned'; donor: SessionId; pane: SessionId }
  | { type: 'forkSpawned'; source: SessionId; pane: SessionId }
  | { type: 'paneExited'; pane: SessionId }
  | { type: 'paneActivated'; pane: SessionId }
  | { type: 'titleChanged'; pane: SessionId; title: string }
  | { type: 'paneColored'; pane: SessionId; color: RowColor | null }
  | { type: 'activeColorToggled'; color: RowColor }
  | { type: 'dropped'; dragged: Dragged; drop: Drop; newGroup: GroupId }
  | { type: 'dividerMoved'; groupIndex: number; dividerIndex: number; leftWidth: number }
  | { type: 'tabGroupCreated'; entry: GroupId; id: TabGroupId }
  | { type: 'tabGroupRenamed'; id: TabGroupId; name: string }
  | { type: 'tabGroupRecolored'; id: TabGroupId; color: RowColor }
  | { type: 'tabGroupUngrouped'; id: TabGroupId }
  | { type: 'tabGroupToggled'; id: TabGroupId }
  | { type: 'agentSignalled'; signal: AgentSignalEvent }
  | { type: 'attentionRequested'; pane: SessionId }
  | { type: 'appActivated' }
  | { type: 'focusRequested'; pane: SessionId }

export function initialWorkspaceState(): WorkspaceState {
  return {
    workspace: null,
    agentRows: new Map(),
    waiting: null,
    rawTitles: new Map(),
    refocus: 0
  }
}


function onScreenPanesOf(workspace: Workspace | null): SessionId[] {
  if (!workspace) {
    return []
  }
  const group = workspace.groups[workspace.activeGroup]
  if (!group) {
    return []
  }
  return group.panes.map((pane) => pane.id)
}

function baseWorkspace(workspace: Workspace | null): Workspace {
  if (workspace === null) {
    return emptyWorkspace()
  }
  return workspace
}

// Changes before the first terminal exists are dropped: there is nothing yet
// for them to describe.
function workspaceEdit(
  state: WorkspaceState,
  change: (workspace: Workspace) => Workspace
): WorkspaceState {
  if (state.workspace === null) {
    return state
  }
  const workspace = change(state.workspace)
  return { ...state, workspace }
}

function refocusingEdit(
  state: WorkspaceState,
  change: (workspace: Workspace) => Workspace
): WorkspaceState {
  const edited = workspaceEdit(state, change)
  if (edited.workspace === state.workspace) {
    return edited
  }
  return { ...edited, refocus: edited.refocus + 1 }
}

function withRawTitle(state: WorkspaceState, pane: SessionId, title: string): WorkspaceState {
  if (state.rawTitles.get(pane) === title) {
    return state
  }
  const rawTitles = new Map(state.rawTitles)
  rawTitles.set(pane, title)
  return { ...state, rawTitles }
}

function withoutRawTitle(state: WorkspaceState, pane: SessionId): WorkspaceState {
  if (!state.rawTitles.has(pane)) {
    return state
  }
  const rawTitles = new Map(state.rawTitles)
  rawTitles.delete(pane)
  return { ...state, rawTitles }
}

function titleRead(state: WorkspaceState, pane: SessionId, title: string): WorkspaceState {
  const row = state.agentRows.get(pane)
  if (row === undefined) {
    return workspaceEdit(state, (workspace) => renamePane(workspace, pane, title))
  }

  const parsed = agentTitleOf(row.harness, title)
  const renamed = workspaceEdit(state, (workspace) => renamePane(workspace, pane, parsed.name))
  if (!parsed.idle) {
    return renamed
  }

  const agentRows = rowsAfterIdleTitle(renamed.agentRows, pane)
  return { ...renamed, agentRows }
}

function titleChanged(state: WorkspaceState, pane: SessionId, title: string): WorkspaceState {
  const stored = withRawTitle(state, pane, title)
  return titleRead(stored, pane, title)
}

function paneExited(state: WorkspaceState, pane: SessionId): WorkspaceState {
  const withoutTitle = withoutRawTitle(state, pane)
  const closed = workspaceEdit(withoutTitle, (workspace) => closePane(workspace, pane))
  const agentRows = rowsAfterExit(closed.agentRows, pane)
  const waiting = waitingAfterExit(closed.waiting, pane)
  return { ...closed, agentRows, waiting }
}

function agentSignalled(state: WorkspaceState, event: AgentSignalEvent): WorkspaceState {
  const onScreenPanes = onScreenPanesOf(state.workspace)
  const onScreen = onScreenPanes.includes(event.id)
  const agentRows = rowsAfterSignal(
    state.agentRows,
    event.id,
    event.harness,
    event.sessionId,
    event.signal,
    onScreen
  )
  const signalled = { ...state, agentRows }

  const startsSession = event.signal === 'start' || event.signal === 'resume'
  if (!startsSession) {
    return signalled
  }

  // A title can arrive before the start signal creates the row, so the last one is read again now.
  const rawTitle = signalled.rawTitles.get(event.id)
  if (rawTitle === undefined) {
    return signalled
  }
  return titleRead(signalled, event.id, rawTitle)
}

function attentionRequested(state: WorkspaceState, pane: SessionId): WorkspaceState {
  const active = activeSessionOf(state.workspace)
  const waiting = waitingAfterAttention(state.waiting, pane, active)
  return { ...state, waiting }
}

function appActivated(state: WorkspaceState): WorkspaceState {
  const waiting = state.waiting
  if (waiting === null) {
    return state
  }
  const activated = workspaceEdit(state, (workspace) => activatePane(workspace, waiting))
  return { ...activated, refocus: activated.refocus + 1 }
}

function focusRequested(state: WorkspaceState, pane: SessionId): WorkspaceState {
  const activated = workspaceEdit(state, (workspace) => activatePane(workspace, pane))
  return { ...activated, refocus: activated.refocus + 1 }
}

function eventApplied(state: WorkspaceState, event: WorkspaceEvent): WorkspaceState {
  switch (event.type) {
    case 'terminalOpened': {
      const base = baseWorkspace(state.workspace)
      const workspace = addGroup(base, event.group)
      return { ...state, workspace }
    }
    case 'terminalOpenedIn': {
      const base = baseWorkspace(state.workspace)
      const workspace = addGroupTo(base, event.group, event.tabGroup)
      return { ...state, workspace }
    }
    case 'workspaceRestored':
      return { ...state, workspace: event.workspace }
    case 'splitSpawned':
      return workspaceEdit(state, (workspace) => splitPane(workspace, event.donor, event.pane))
    case 'forkSpawned':
      return workspaceEdit(state, (workspace) => forkPane(workspace, event.source, event.pane))
    case 'paneExited':
      return paneExited(state, event.pane)
    case 'paneActivated':
      return workspaceEdit(state, (workspace) => activatePane(workspace, event.pane))
    case 'titleChanged':
      return titleChanged(state, event.pane, event.title)
    case 'paneColored':
      return workspaceEdit(state, (workspace) => setPaneColor(workspace, event.pane, event.color))
    case 'activeColorToggled':
      return workspaceEdit(state, (workspace) => toggleActivePaneColor(workspace, event.color))
    case 'dropped':
      return refocusingEdit(state, (workspace) => {
        return applyDrop(workspace, event.dragged, event.drop, event.newGroup)
      })
    case 'dividerMoved':
      return workspaceEdit(state, (workspace) => {
        return moveDivider(workspace, event.groupIndex, event.dividerIndex, event.leftWidth)
      })
    case 'tabGroupCreated':
      return workspaceEdit(state, (workspace) => createTabGroup(workspace, event.entry, event.id))
    case 'tabGroupRenamed':
      return workspaceEdit(state, (workspace) => renameTabGroup(workspace, event.id, event.name))
    case 'tabGroupRecolored':
      return workspaceEdit(state, (workspace) => recolorTabGroup(workspace, event.id, event.color))
    case 'tabGroupUngrouped':
      return workspaceEdit(state, (workspace) => ungroupTabGroup(workspace, event.id))
    case 'tabGroupToggled':
      return refocusingEdit(state, (workspace) => toggleTabGroup(workspace, event.id))
    case 'agentSignalled':
      return agentSignalled(state, event.signal)
    case 'attentionRequested':
      return attentionRequested(state, event.pane)
    case 'appActivated':
      return appActivated(state)
    case 'focusRequested':
      return focusRequested(state, event.pane)
  }
}

// What is on screen and which pane is active follow from the workspace, so both are settled on every change to it.
function settledAfterWorkspaceChange(before: WorkspaceState, after: WorkspaceState): WorkspaceState {
  if (after.workspace === before.workspace) {
    return after
  }

  const onScreenPanes = onScreenPanesOf(after.workspace)
  const agentRows = rowsAfterScreen(after.agentRows, onScreenPanes)
  const active = activeSessionOf(after.workspace)
  const waiting = waitingAfterFocus(after.waiting, active)
  return { ...after, agentRows, waiting }
}

function sameSlices(a: WorkspaceState, b: WorkspaceState): boolean {
  return (
    a.workspace === b.workspace &&
    a.agentRows === b.agentRows &&
    a.waiting === b.waiting &&
    a.rawTitles === b.rawTitles &&
    a.refocus === b.refocus
  )
}

// Raw titles are bookkeeping for the title replay and are never drawn, so a change to them alone is not worth a render.
export function drawsTheSame(a: WorkspaceState, b: WorkspaceState): boolean {
  return (
    a.workspace === b.workspace &&
    a.agentRows === b.agentRows &&
    a.waiting === b.waiting &&
    a.refocus === b.refocus
  )
}

export function reduceWorkspaceState(
  state: WorkspaceState,
  event: WorkspaceEvent
): WorkspaceState {
  const applied = eventApplied(state, event)
  const settled = settledAfterWorkspaceChange(state, applied)
  if (sameSlices(state, settled)) {
    return state
  }
  return settled
}
