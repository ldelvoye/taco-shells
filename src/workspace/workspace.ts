import { DEFAULT_HARNESS, HARNESSES } from '@agents/registry'
import type { Harness } from '@agents/types'
import type { Group, GroupId, TabGroupId, Workspace as Arrangement } from '@protocol/arrangement'
import type { AgentSignalEvent, SessionId } from '@protocol/ipc'
import type { Snapshot } from '@protocol/snapshot'
import type {
  AgentLabel,
  DrawnState,
  RequestReply,
  WorkspaceRequest
} from '@protocol/workspace'
import { canSplitPane, emptyWorkspace } from '@rules/arrangement'
import { workspaceFromSnapshot } from './snapshot/format'
import { createWorkspaceStore, type WorkspaceStore } from './store'
import type { WorkspaceEvent } from './workspaceState'

export interface WorkspaceEffects {
  onState(state: DrawnState): void
  onArrangement(arrangement: Arrangement): void
  onEmptied(): void
}

type Answer = Omit<RequestReply, 'revision'>

function accepted(tabGroup: TabGroupId | null): Answer {
  return { ok: true, tabGroup }
}

function refused(): Answer {
  return { ok: false, tabGroup: null }
}

function groupForSession(id: GroupId, sessionId: SessionId): Group {
  return {
    id,
    panes: [{ id: sessionId, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function entryHolding(arrangement: Arrangement, pane: SessionId): Group | undefined {
  return arrangement.groups.find((group) => {
    return group.panes.some((candidate) => candidate.id === pane)
  })
}

function hasGroups(arrangement: Arrangement | null): boolean {
  if (arrangement === null) {
    return false
  }
  return arrangement.groups.length > 0
}

function hasNoGroups(arrangement: Arrangement | null): boolean {
  if (arrangement === null) {
    return false
  }
  return arrangement.groups.length === 0
}

function agentLabelsOf(harnesses: readonly Harness[]): Record<string, AgentLabel> {
  const labels: Record<string, AgentLabel> = {}
  for (const harness of harnesses) {
    const forks = harness.forkCommand !== undefined
    labels[harness.id] = { name: harness.name, forks }
  }
  return labels
}

export class Workspace {
  private readonly store: WorkspaceStore = createWorkspaceStore()
  private readonly snapshot: Snapshot | null
  private readonly effects: WorkspaceEffects
  private readonly agents: Record<string, AgentLabel>
  private revision = 0
  private focusMove = 0
  private nextGroupNumber = 1
  private nextTabGroupNumber = 1

  constructor(snapshot: Snapshot | null, effects: WorkspaceEffects) {
    this.snapshot = snapshot
    this.effects = effects
    this.agents = agentLabelsOf(HARNESSES)
  }

  drawn(): DrawnState {
    const state = this.store.getState()
    const agentRows = Object.fromEntries(state.agentRows)
    return {
      revision: this.revision,
      workspace: state.workspace,
      agentRows,
      waiting: state.waiting,
      refocus: state.refocus,
      focusMove: this.focusMove,
      agents: this.agents,
      defaultAgentName: DEFAULT_HARNESS.name
    }
  }

  request(request: WorkspaceRequest): RequestReply {
    const answer = this.answer(request)
    return { ...answer, revision: this.revision }
  }

  private answer(request: WorkspaceRequest): Answer {
    switch (request.type) {
      case 'terminalOpened': {
        const group = this.groupFor(request.pane)
        this.dispatch({ type: 'terminalOpened', group }, false)
        return accepted(null)
      }
      case 'terminalOpenedIn': {
        const group = this.groupFor(request.pane)
        this.dispatch({ type: 'terminalOpenedIn', group, tabGroup: request.tabGroup }, false)
        return accepted(null)
      }
      case 'restored':
        return this.restore(request.panes)
      case 'splitSpawned': {
        if (!this.canSplit(request.donor)) {
          return refused()
        }
        this.dispatch(request, false)
        return accepted(null)
      }
      case 'forkSpawned': {
        if (!this.canSplit(request.source)) {
          return refused()
        }
        this.dispatch(request, false)
        return accepted(null)
      }
      case 'focusReported': {
        let advanced = false
        if (request.move > this.focusMove) {
          this.focusMove = request.move
          advanced = true
        }
        this.dispatch({ type: 'paneActivated', pane: request.pane }, advanced)
        return accepted(null)
      }
      case 'dropped': {
        const newGroup = this.nextGroupId()
        this.dispatch(
          { type: 'dropped', dragged: request.dragged, drop: request.drop, newGroup },
          false
        )
        return accepted(null)
      }
      case 'tabGroupCreated':
        return this.createTabGroup(request.pane)
      case 'titleChanged':
      case 'paneColored':
      case 'activeColorToggled':
      case 'dividerMoved':
      case 'tabGroupRenamed':
      case 'tabGroupRecolored':
      case 'tabGroupUngrouped':
      case 'tabGroupToggled':
        this.dispatch(request, false)
        return accepted(null)
    }
  }

  terminalExited(pane: SessionId): void {
    this.dispatch({ type: 'paneExited', pane }, false)
  }

  agentSignalled(event: AgentSignalEvent): void {
    this.dispatch({ type: 'agentSignalled', signal: event }, false)
  }

  attentionRequested(pane: SessionId): void {
    this.dispatch({ type: 'attentionRequested', pane }, false)
  }

  focusRequested(pane: SessionId): void {
    this.dispatch({ type: 'focusRequested', pane }, false)
  }

  appActivated(): void {
    this.dispatch({ type: 'appActivated' }, false)
  }

  private nextGroupId(): GroupId {
    const id = `group-${this.nextGroupNumber}`
    this.nextGroupNumber += 1
    return id
  }

  private nextTabGroupId(): TabGroupId {
    const id = `tab-group-${this.nextTabGroupNumber}`
    this.nextTabGroupNumber += 1
    return id
  }

  private groupFor(sessionId: SessionId): Group {
    const id = this.nextGroupId()
    return groupForSession(id, sessionId)
  }

  private canSplit(donor: SessionId): boolean {
    const arrangement = this.store.getState().workspace
    if (arrangement === null) {
      return false
    }
    return canSplitPane(arrangement, donor)
  }

  private createTabGroup(pane: SessionId): Answer {
    const arrangement = this.store.getState().workspace
    if (arrangement === null) {
      return accepted(null)
    }

    const holder = entryHolding(arrangement, pane)
    if (holder === undefined) {
      return accepted(null)
    }

    const id = this.nextTabGroupId()
    this.dispatch({ type: 'tabGroupCreated', entry: holder.id, id }, false)
    return accepted(id)
  }

  private restore(panes: SessionId[]): Answer {
    const snapshot = this.snapshot
    if (snapshot === null) {
      return refused()
    }

    let restored: Arrangement = emptyWorkspace()
    try {
      const groupIds = snapshot.groups.map(() => this.nextGroupId())
      const tabGroupIds = snapshot.tabGroups.map(() => this.nextTabGroupId())
      restored = workspaceFromSnapshot(snapshot, panes, groupIds, tabGroupIds)
    } catch {
      restored = emptyWorkspace()
    }

    if (restored.groups.length === 0) {
      return refused()
    }
    this.dispatch({ type: 'workspaceRestored', workspace: restored }, false)
    return accepted(null)
  }

  private dispatch(event: WorkspaceEvent, focusAdvanced: boolean): void {
    const before = this.store.getState()
    const drawnBefore = this.store.getDrawnState()

    this.store.dispatch(event)

    const after = this.store.getState()
    const drawnChanged = this.store.getDrawnState() !== drawnBefore
    if (drawnChanged || focusAdvanced) {
      this.revision += 1
      this.effects.onState(this.drawn())
    }

    const arrangement = after.workspace
    if (arrangement !== null && arrangement !== before.workspace) {
      this.effects.onArrangement(arrangement)
    }

    const wasOccupied = hasGroups(before.workspace)
    const isEmpty = hasNoGroups(after.workspace)
    if (wasOccupied && isEmpty) {
      this.effects.onEmptied()
    }
  }
}
