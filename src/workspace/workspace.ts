import { DEFAULT_HARNESS, HARNESSES } from '@agents/registry'
import type { Harness } from '@agents/types'
import type { Group, GroupId, TabGroupId, Workspace as Arrangement } from '@protocol/arrangement'
import type { ConfigFile } from '@protocol/config'
import type { AgentSignalEvent, PtySize, SessionId } from '@protocol/ipc'
import type { AgentSignal } from '@protocol/rows'
import type { Runtime } from '@protocol/runtime'
import type {
  AgentLabel,
  DrawnState,
  RequestReply,
  WorkspaceRequest
} from '@protocol/workspace'
import { canSplitPane, emptyWorkspace, locatePane } from '@rules/arrangement'
import { forkNameFor } from '@rules/forkName'
import type { AgentSessions } from './agentSessions'
import {
  workspaceFromSnapshot,
  type AgentSession,
  type Snapshot,
  type SnapshotPane
} from './snapshot/format'
import { shellStartFor, type ShellRequest, type ShellStart } from './startCommand'
import { createWorkspaceStore, type WorkspaceStore } from './store'
import type { WorkspaceEvent } from './workspaceState'

const INITIAL_SIZE: PtySize = { cols: 80, rows: 24 }

// The requests that would start a shell with no terminal to start from. A split or a
// fork needs a terminal that is there, and once the runtime has died none is.
const OPENING_REQUESTS: WorkspaceRequest['type'][] = ['openTerminal', 'openTerminalIn']

export interface WorkspaceOptions {
  snapshot: Snapshot | null
  runtime: Runtime
  configPathOf(file: ConfigFile): string
  agentSessions: AgentSessions
}

export interface WorkspaceEffects {
  onState(state: DrawnState): void
  onArrangement(arrangement: Arrangement): void
  onEmptied(): void
  onStartedIn(id: SessionId, directory: string): void
  onExited(id: SessionId): void
}

type Answer = Omit<RequestReply, 'revision'>

function accepted(tabGroup: TabGroupId | null): Answer {
  return { ok: true, tabGroup }
}

function refused(): Answer {
  return { ok: false, tabGroup: null }
}

function runtimeProblemFor(reason: string): string {
  return `The terminals stopped because their background process exited (${reason}). Quit and reopen Taco Shells to start again.`
}

function groupForSession(id: GroupId, sessionId: SessionId): Group {
  return {
    id,
    panes: [{ id: sessionId, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

// The shell resolves EDITOR, not us: on a GUI launch the app's own environment
// does not have it, because it is set in the files an interactive shell reads.
function editorCommand(path: string): string {
  return `\${EDITOR:-vi} '${path}'\n`
}

function startOptionsOf(pane: SnapshotPane): ShellRequest {
  if (pane.agent !== null) {
    return { resume: pane.agent }
  } else if (typeof pane.cwd === 'string') {
    return { cwd: pane.cwd }
  } else {
    return {}
  }
}

function entryHolding(arrangement: Arrangement, pane: SessionId): Group | undefined {
  return arrangement.groups.find((group) => {
    return group.panes.some((candidate) => candidate.id === pane)
  })
}

function groupCountOf(arrangement: Arrangement | null): number | null {
  if (arrangement === null) {
    return null
  }
  return arrangement.groups.length
}

function agentLabelsOf(harnesses: readonly Harness[]): Record<string, AgentLabel> {
  const labels: Record<string, AgentLabel> = {}
  for (const harness of harnesses) {
    const forks = harness.forkCommand !== undefined
    labels[harness.id] = { name: harness.name, forks }
  }
  return labels
}

/** The owner of the arrangement: it applies what the window asks for and what the app reports, and publishes what to draw. */
export class Workspace {
  private readonly store: WorkspaceStore = createWorkspaceStore()
  private readonly options: WorkspaceOptions
  private readonly effects: WorkspaceEffects
  private readonly agents: Record<string, AgentLabel>
  private revision = 0
  private focusCount = 0
  private publishedFocusCount = 0
  private nextTerminalNumber = 1
  private nextGroupNumber = 1
  private nextTabGroupNumber = 1
  private problem: string | null = null
  private runtimeGone = false

  constructor(options: WorkspaceOptions, effects: WorkspaceEffects) {
    this.options = options
    this.effects = effects
    this.agents = agentLabelsOf(HARNESSES)
  }

  /** Restores the saved arrangement, or opens the first terminal. Called once, after the effects' receivers exist. */
  start(): void {
    this.options.runtime.onExit((event) => {
      this.terminalExited(event.id)
    })
    this.options.runtime.onTitle((id, title) => {
      this.terminalTitled(id, title)
    })
    this.options.runtime.onStarted((event) => {
      this.effects.onStartedIn(event.id, event.directory)
    })

    const snapshot = this.options.snapshot
    if (snapshot !== null) {
      const restored = this.restore(snapshot)
      if (restored) {
        return
      }
    }
    this.openFreshTerminal(null)
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
      focusCount: this.focusCount,
      agents: this.agents,
      defaultAgentName: DEFAULT_HARNESS.name,
      problem: this.problem
    }
  }

  runtimeDied(reason: string): void {
    this.runtimeGone = true
    this.problem = runtimeProblemFor(reason)

    const arrangement = this.store.getState().workspace
    let open: SessionId[] = []
    if (arrangement !== null) {
      const panes = arrangement.groups.flatMap((group) => group.panes)
      open = panes.map((pane) => pane.id)
    }
    // No shell is left to report its own exit, so each terminal goes the way an exited one does.
    for (const pane of open) {
      this.terminalExited(pane)
    }

    this.revision += 1
    this.effects.onState(this.drawn())
  }

  request(request: WorkspaceRequest): RequestReply {
    const answer = this.answer(request)
    return { ...answer, revision: this.revision }
  }

  agentStarted(pane: SessionId, session: AgentSession, hasConversation: boolean): void {
    this.options.agentSessions.recordStart(pane, session)
    let signal: AgentSignal = 'start'
    if (hasConversation) {
      signal = 'resume'
    }
    const event: AgentSignalEvent = { id: pane, harness: session.harness, sessionId: session.sessionId, signal }
    this.agentSignalled(event)
  }

  agentEnded(pane: SessionId, harness: string, sessionId: string): void {
    this.options.agentSessions.recordEnd(pane, harness, sessionId)
    const event: AgentSignalEvent = { id: pane, harness, sessionId, signal: 'end' }
    this.agentSignalled(event)
  }

  agentSignalled(event: AgentSignalEvent): void {
    this.dispatch({ type: 'agentSignalled', signal: event })
  }

  attentionRequested(pane: SessionId): void {
    if (!this.holdsPane(pane)) {
      return
    }
    this.dispatch({ type: 'attentionRequested', pane })
  }

  focusRequested(pane: SessionId): void {
    this.dispatch({ type: 'focusRequested', pane })
  }

  appActivated(): void {
    this.dispatch({ type: 'appActivated' })
  }

  private answer(request: WorkspaceRequest): Answer {
    if (this.runtimeGone && OPENING_REQUESTS.includes(request.type)) {
      return refused()
    }
    switch (request.type) {
      case 'openTerminal': {
        let command: string | null = null
        if (request.file !== null) {
          const path = this.options.configPathOf(request.file)
          command = editorCommand(path)
        }
        this.openFreshTerminal(command)
        return accepted(null)
      }
      case 'openTerminalIn': {
        const pane = this.nextTerminalId()
        const group = this.groupFor(pane)
        this.dispatch({ type: 'terminalOpenedIn', group, tabGroup: request.tabGroup })
        this.openTerminal(pane, { cwd: null, cwdFrom: null, command: null })
        return accepted(null)
      }
      case 'splitTerminal': {
        if (!this.canSplit(request.pane)) {
          return refused()
        }
        const pane = this.nextTerminalId()
        const shell = this.shellStart({ cwdFrom: request.pane })
        this.dispatch({ type: 'splitSpawned', donor: request.pane, pane })
        this.openTerminal(pane, shell)
        return accepted(null)
      }
      case 'forkTerminal':
        return this.fork(request.pane)
      case 'closeTerminal':
        if (this.holdsPane(request.pane)) {
          this.options.runtime.close(request.pane)
        }
        return accepted(null)
      case 'focusReported': {
        // Recorded even when the terminal was already active: the window keeps its own
        // focus move on screen until the published count reaches it.
        if (request.count > this.focusCount) {
          this.focusCount = request.count
        }
        this.dispatch({ type: 'paneActivated', pane: request.pane })
        return accepted(null)
      }
      case 'dropped': {
        const newGroup = this.nextGroupId()
        this.dispatch({ type: 'dropped', dragged: request.dragged, drop: request.drop, newGroup })
        return accepted(null)
      }
      case 'tabGroupCreated':
        return this.createTabGroup(request.pane)
      case 'paneColored':
      case 'activeColorToggled':
      case 'dividerMoved':
      case 'tabGroupRenamed':
      case 'tabGroupRecolored':
      case 'tabGroupUngrouped':
      case 'tabGroupToggled':
        this.dispatch(request)
        return accepted(null)
    }
  }

  private holdsPane(pane: SessionId): boolean {
    const workspace = this.store.getState().workspace
    if (workspace === null) {
      return false
    }
    return locatePane(workspace, pane) !== null
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

  private nextTerminalId(): SessionId {
    const id = `pty-${this.nextTerminalNumber}`
    this.nextTerminalNumber += 1
    return id
  }

  private shellStart(options: ShellRequest): ShellStart {
    const agentSessions = this.options.agentSessions
    return shellStartFor(options, (id) => agentSessions.sessionOf(id))
  }

  private openTerminal(id: SessionId, shell: ShellStart): void {
    this.options.runtime.open({
      id,
      directory: shell.cwd,
      directoryOf: shell.cwdFrom,
      command: shell.command,
      size: INITIAL_SIZE
    })
  }

  private openFreshTerminal(command: string | null): void {
    const pane = this.nextTerminalId()
    const group = this.groupFor(pane)
    this.dispatch({ type: 'terminalOpened', group })
    this.openTerminal(pane, { cwd: null, cwdFrom: null, command })
  }

  private terminalTitled(pane: SessionId, title: string): void {
    const workspace = this.store.getState().workspace
    if (workspace === null || locatePane(workspace, pane) === null) {
      return
    }
    this.dispatch({ type: 'titleChanged', pane, title })
  }

  private terminalExited(pane: SessionId): void {
    this.options.agentSessions.forget(pane)
    this.effects.onExited(pane)
    this.dispatch({ type: 'paneExited', pane })
  }

  private fork(source: SessionId): Answer {
    const arrangement = this.store.getState().workspace
    if (arrangement === null) {
      return refused()
    }
    if (!canSplitPane(arrangement, source)) {
      return refused()
    }

    const panes = arrangement.groups.flatMap((group) => group.panes)
    const sourcePane = panes.find((pane) => pane.id === source)
    if (sourcePane === undefined) {
      return refused()
    }
    const titles = panes.map((pane) => pane.title)
    const name = forkNameFor(sourcePane.title, titles)

    const pane = this.nextTerminalId()
    const shell = this.shellStart({ fork: { from: source, name } })
    this.dispatch({ type: 'forkSpawned', source, pane })
    this.openTerminal(pane, shell)
    return accepted(null)
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
    this.dispatch({ type: 'tabGroupCreated', entry: holder.id, id })
    return accepted(id)
  }

  private restore(snapshot: Snapshot): boolean {
    // Ids are handed out in group order, the order workspaceFromSnapshot takes them in.
    const savedPanes = snapshot.groups.flatMap((group) => group.panes)
    const paneIds = savedPanes.map(() => this.nextTerminalId())

    let restored: Arrangement = emptyWorkspace()
    try {
      const groupIds = snapshot.groups.map(() => this.nextGroupId())
      const tabGroupIds = snapshot.tabGroups.map(() => this.nextTabGroupId())
      restored = workspaceFromSnapshot(snapshot, paneIds, groupIds, tabGroupIds)
    } catch {
      restored = emptyWorkspace()
    }

    if (restored.groups.length === 0) {
      return false
    }

    const shells = savedPanes.map((savedPane, index) => this.savedShell(paneIds[index], savedPane))
    this.dispatch({ type: 'workspaceRestored', workspace: restored })
    shells.forEach((shell, index) => {
      this.openTerminal(paneIds[index], shell)
    })
    return true
  }

  // What a saved pane resumes and where it was are recorded before its shell exists:
  // the first save after a restore can come before any shell has started.
  private savedShell(id: SessionId, saved: SnapshotPane): ShellStart {
    const options = startOptionsOf(saved)
    const shell = this.shellStart(options)
    if (saved.agent !== null) {
      this.options.agentSessions.seed(id, saved.agent)
    }
    if (shell.cwd !== null) {
      this.effects.onStartedIn(id, shell.cwd)
    }
    return shell
  }

  private dispatch(event: WorkspaceEvent): void {
    const before = this.store.getState()
    const drawnBefore = this.store.getDrawnState()

    this.store.dispatch(event)

    const after = this.store.getState()
    const drawnChanged = this.store.getDrawnState() !== drawnBefore
    const focusCounted = this.focusCount !== this.publishedFocusCount
    if (drawnChanged || focusCounted) {
      this.revision += 1
      this.publishedFocusCount = this.focusCount
      this.effects.onState(this.drawn())
    }

    const arrangement = after.workspace
    if (arrangement !== null && arrangement !== before.workspace) {
      this.effects.onArrangement(arrangement)
    }

    const groupsBefore = groupCountOf(before.workspace)
    const groupsAfter = groupCountOf(after.workspace)
    const lastOneWent = groupsBefore !== null && groupsBefore > 0 && groupsAfter === 0
    // Emptying ends the app, and after the runtime died the window has to stay to say so.
    if (lastOneWent && !this.runtimeGone) {
      this.effects.onEmptied()
    }
  }
}
