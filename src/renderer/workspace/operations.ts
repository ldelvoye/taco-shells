import { forkNameFor } from '@shared/forkName'
import type { PtyCreateOptions, SessionId } from '@shared/ipc'
import {
  canSplitPane,
  type Dragged,
  type Drop,
  emptyWorkspace,
  type Group,
  type RowColor,
  type TabGroupId,
  type Workspace
} from '@shared/model'
import { type Snapshot, type SnapshotPane, workspaceFromSnapshot } from '@shared/snapshot'
import { activeSessionOf, type FocusMove, type WorkspaceEvent } from '@shared/workspaceState'
import {
  createSession,
  disposeSession,
  focusSession,
  onSessionFocus,
  onSessionTitle
} from '../terminal/registry'
import type { WorkspaceStore } from './store'

let nextGroupNumber = 1

let nextTabGroupNumber = 1

export interface WorkspaceOperations {
  start(): void
  openTerminal(): void
  openTerminalIn(tabGroup: TabGroupId | null): void
  openSettings(): void
  openKeybindings(): void
  splitActive(): void
  fork(source: SessionId): void
  closeActive(): void
  close(pane: SessionId): void
  select(pane: SessionId): void
  moveFocus(move: FocusMove): void
  colorPane(pane: SessionId, color: RowColor | null): void
  toggleActiveColor(color: RowColor): void
  drop(dragged: Dragged, drop: Drop): void
  resizeGroup(groupIndex: number, dividerIndex: number, leftWidth: number): void
  newTabGroup(pane: SessionId): TabGroupId | null
  renameTabGroup(id: TabGroupId, name: string): void
  recolorTabGroup(id: TabGroupId, color: RowColor): void
  ungroupTabGroup(id: TabGroupId): void
  toggleTabGroup(id: TabGroupId): void
  refocusActive(): void
  activeSession(): SessionId | null
  listen(): () => void
}

function nextGroupId(): string {
  const id = `group-${nextGroupNumber}`
  nextGroupNumber += 1
  return id
}

function nextTabGroupId(): string {
  const id = `tab-group-${nextTabGroupNumber}`
  nextTabGroupNumber += 1
  return id
}

function groupForSession(sessionId: SessionId): Group {
  const id = nextGroupId()
  return {
    id,
    panes: [{ id: sessionId, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function entryHolding(workspace: Workspace, pane: SessionId): Group | undefined {
  return workspace.groups.find((group) => {
    return group.panes.some((candidate) => candidate.id === pane)
  })
}

function startOptionsOf(pane: SnapshotPane): PtyCreateOptions {
  if (pane.agent !== null) {
    return { resume: pane.agent }
  } else if (typeof pane.cwd === 'string') {
    return { cwd: pane.cwd }
  } else {
    return {}
  }
}

// The shell resolves EDITOR, not us: on a GUI launch the app's own environment
// does not have it, because it is set in the files an interactive shell reads.
function editorCommand(path: string): string {
  return `\${EDITOR:-vi} '${path}'\n`
}

export function createWorkspaceOperations(store: WorkspaceStore): WorkspaceOperations {
  function activeSession(): SessionId | null {
    const workspace = store.getState().workspace
    return activeSessionOf(workspace)
  }

  function openTerminalPlaced(
    openedEvent: (group: Group) => WorkspaceEvent,
    command: string | null
  ): void {
    void createSession().then((sessionId) => {
      const group = groupForSession(sessionId)
      const event = openedEvent(group)
      store.dispatch(event)
      if (command !== null) {
        window.tacoShells.pty.write(sessionId, command)
      }
    })
  }

  function openTerminalRunning(command: string | null): void {
    openTerminalPlaced((group) => {
      return { type: 'terminalOpened', group }
    }, command)
  }

  function openTerminal(): void {
    openTerminalRunning(null)
  }

  function openTerminalIn(tabGroup: TabGroupId | null): void {
    openTerminalPlaced((group) => {
      return { type: 'terminalOpenedIn', group, tabGroup }
    }, null)
  }

  function openSettings(): void {
    const path = window.tacoShells.config.pathOf('settings')
    const command = editorCommand(path)
    openTerminalRunning(command)
  }

  function openKeybindings(): void {
    const path = window.tacoShells.config.pathOf('keybindings')
    const command = editorCommand(path)
    openTerminalRunning(command)
  }

  // The donor can close, or its group fill, while the shell is spawning: a shell
  // with no pane to show it would keep running unseen.
  function splitOnceSpawned(
    donor: SessionId,
    options: PtyCreateOptions,
    spawnedEvent: (pane: SessionId) => WorkspaceEvent
  ): void {
    void createSession(options).then((sessionId) => {
      const latest = store.getState().workspace
      const stillSplittable = latest !== null && canSplitPane(latest, donor)
      if (!stillSplittable) {
        disposeSession(sessionId)
        return
      }
      const event = spawnedEvent(sessionId)
      store.dispatch(event)
    })
  }

  // The new shell starts in the split terminal's directory, which the main
  // process resolves from the pty it names here.
  function splitActive(): void {
    const workspace = store.getState().workspace
    const activeSessionId = activeSessionOf(workspace)
    if (!activeSessionId) {
      return
    }
    if (!workspace) {
      return
    }
    if (!canSplitPane(workspace, activeSessionId)) {
      return
    }

    splitOnceSpawned(activeSessionId, { cwdFrom: activeSessionId }, (pane) => {
      return { type: 'splitSpawned', donor: activeSessionId, pane }
    })
  }

  // The main process finds the agent session to fork from the pane named here.
  function fork(source: SessionId): void {
    const workspace = store.getState().workspace
    if (!workspace) {
      return
    }
    if (!canSplitPane(workspace, source)) {
      return
    }

    const panes = workspace.groups.flatMap((group) => group.panes)
    const sourcePane = panes.find((pane) => pane.id === source)
    if (!sourcePane) {
      return
    }
    const titles = panes.map((pane) => pane.title)
    const name = forkNameFor(sourcePane.title, titles)

    splitOnceSpawned(source, { fork: { from: source, name } }, (pane) => {
      return { type: 'forkSpawned', source, pane }
    })
  }

  // Panes are created in group order and workspaceFromSnapshot hands the ids
  // back out in that same order, so the two loops have to walk the same list.
  async function restore(snapshot: Snapshot): Promise<void> {
    const sessionIds: SessionId[] = []
    let restored: Workspace = emptyWorkspace()
    try {
      for (const group of snapshot.groups) {
        for (const pane of group.panes) {
          const startOptions = startOptionsOf(pane)
          const sessionId = await createSession(startOptions)
          sessionIds.push(sessionId)
        }
      }
      const groupIds = snapshot.groups.map(() => nextGroupId())
      const tabGroupIds = snapshot.tabGroups.map(() => nextTabGroupId())
      restored = workspaceFromSnapshot(snapshot, sessionIds, groupIds, tabGroupIds)
    } catch {
      restored = emptyWorkspace()
    }

    if (restored.groups.length === 0) {
      for (const sessionId of sessionIds) {
        disposeSession(sessionId)
      }
      openTerminal()
      return
    }
    store.dispatch({ type: 'workspaceRestored', workspace: restored })
  }

  function start(): void {
    const initial = window.tacoShells.snapshot.initial
    if (initial === null || initial.groups.length === 0) {
      openTerminal()
      return
    }
    void restore(initial)
  }

  function closeActive(): void {
    const activeSessionId = activeSession()
    if (!activeSessionId) {
      return
    }
    disposeSession(activeSessionId)
  }

  function close(pane: SessionId): void {
    disposeSession(pane)
  }

  // Focused here rather than left to the focus effect in useWorkspace, because clicking the row
  // of the pane that is already active changes no state and so would fire no
  // effect, while the click itself has already taken focus off the terminal.
  function select(pane: SessionId): void {
    store.dispatch({ type: 'paneActivated', pane })
    focusSession(pane)
  }

  function moveFocus(move: FocusMove): void {
    store.dispatch({ type: 'focusMoved', move })
  }

  function colorPane(pane: SessionId, color: RowColor | null): void {
    store.dispatch({ type: 'paneColored', pane, color })
  }

  function toggleActiveColor(color: RowColor): void {
    store.dispatch({ type: 'activeColorToggled', color })
  }

  function drop(dragged: Dragged, target: Drop): void {
    const newGroup = nextGroupId()
    store.dispatch({ type: 'dropped', dragged, drop: target, newGroup })
  }

  function resizeGroup(groupIndex: number, dividerIndex: number, leftWidth: number): void {
    store.dispatch({ type: 'dividerMoved', groupIndex, dividerIndex, leftWidth })
  }

  function newTabGroup(pane: SessionId): TabGroupId | null {
    const current = store.getState().workspace
    if (current === null) {
      return null
    }

    const holder = entryHolding(current, pane)
    if (holder === undefined) {
      return null
    }

    const id = nextTabGroupId()
    store.dispatch({ type: 'tabGroupCreated', entry: holder.id, id })
    return id
  }

  function renameTabGroup(id: TabGroupId, name: string): void {
    store.dispatch({ type: 'tabGroupRenamed', id, name })
  }

  function recolorTabGroup(id: TabGroupId, color: RowColor): void {
    store.dispatch({ type: 'tabGroupRecolored', id, color })
  }

  function ungroupTabGroup(id: TabGroupId): void {
    store.dispatch({ type: 'tabGroupUngrouped', id })
  }

  function toggleTabGroup(id: TabGroupId): void {
    store.dispatch({ type: 'tabGroupToggled', id })
  }

  function refocusActive(): void {
    const activeSessionId = activeSession()
    if (!activeSessionId) {
      return
    }
    focusSession(activeSessionId)
  }

  function listen(): () => void {
    // A pane leaves the workspace when its pty exits and at no other time, so
    // typing `exit` and closing the terminal from the app take one path.
    const stopExit = window.tacoShells.pty.onExit((event) => {
      disposeSession(event.id)
      store.dispatch({ type: 'paneExited', pane: event.id })
    })

    const stopTitle = onSessionTitle((sessionId, title) => {
      store.dispatch({ type: 'titleChanged', pane: sessionId, title })
    })

    // Clicking straight into a pane has to move the workspace's idea of the active
    // pane with it, or cmd+w would close a terminal the user is not typing in. The focus
    // effect in useWorkspace refocuses on that change, but activatePane returns the
    // workspace untouched once the pane is already active, so this cannot loop.
    const stopFocus = onSessionFocus((sessionId) => {
      store.dispatch({ type: 'paneActivated', pane: sessionId })
    })

    const stopSignal = window.tacoShells.focus.onAgentSignal((event) => {
      store.dispatch({ type: 'agentSignalled', signal: event })
    })

    const stopAttention = window.tacoShells.focus.onAttention((id) => {
      store.dispatch({ type: 'attentionRequested', pane: id })
    })

    const stopActivated = window.tacoShells.focus.onActivated(() => {
      const waiting = store.getState().waiting
      if (waiting === null) {
        return
      }
      store.dispatch({ type: 'appActivated' })
      focusSession(waiting)
    })

    const stopFocusRequest = window.tacoShells.focus.onFocusSession((id) => {
      store.dispatch({ type: 'paneActivated', pane: id })
      focusSession(id)
    })

    return () => {
      stopExit()
      stopTitle()
      stopFocus()
      stopSignal()
      stopAttention()
      stopActivated()
      stopFocusRequest()
    }
  }

  return {
    start,
    openTerminal,
    openTerminalIn,
    openSettings,
    openKeybindings,
    splitActive,
    fork,
    closeActive,
    close,
    select,
    moveFocus,
    colorPane,
    toggleActiveColor,
    drop,
    resizeGroup,
    newTabGroup,
    renameTabGroup,
    recolorTabGroup,
    ungroupTabGroup,
    toggleTabGroup,
    refocusActive,
    activeSession,
    listen
  }
}
