import type { Dragged, Drop, RowColor, TabGroupId } from '@protocol/arrangement'
import type { PtyCreateOptions, SessionId } from '@protocol/ipc'
import type { Snapshot, SnapshotPane } from '@protocol/snapshot'
import type { FocusMove, WorkspaceRequest } from '@protocol/workspace'
import { activeSessionOf, canSplitPane } from '@rules/arrangement'
import { focusAfter } from '@rules/focus'
import { forkNameFor } from '@rules/forkName'
import {
  createSession,
  disposeSession,
  focusSession,
  onSessionFocus,
  onSessionTitle
} from '../terminal/registry'
import type { WorkspaceLink } from './link'

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
  newTabGroup(pane: SessionId): Promise<TabGroupId | null>
  renameTabGroup(id: TabGroupId, name: string): void
  recolorTabGroup(id: TabGroupId, color: RowColor): void
  ungroupTabGroup(id: TabGroupId): void
  toggleTabGroup(id: TabGroupId): void
  refocusActive(): void
  activeSession(): SessionId | null
  listen(): () => void
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

export function createWorkspaceOperations(link: WorkspaceLink): WorkspaceOperations {
  // Nothing waits on these, so a request that fails is reported here or nowhere.
  function send(request: WorkspaceRequest): void {
    link.request(request).catch((failure: unknown) => {
      console.error('a workspace request failed', failure)
    })
  }

  // A shell the workspace did not place has no row to close it from.
  async function placeOrClose(sessionId: SessionId, request: WorkspaceRequest): Promise<void> {
    let placed = false
    try {
      const reply = await link.request(request)
      placed = reply.ok
    } catch {
      placed = false
    }
    if (!placed) {
      disposeSession(sessionId)
    }
  }

  function activeSession(): SessionId | null {
    const workspace = link.getDrawn().workspace
    return activeSessionOf(workspace)
  }

  function openTerminalPlaced(
    openedRequest: (pane: SessionId) => WorkspaceRequest,
    command: string | null
  ): void {
    void createSession().then((sessionId) => {
      const request = openedRequest(sessionId)
      send(request)
      if (command !== null) {
        window.tacoShells.pty.write(sessionId, command)
      }
    })
  }

  function openTerminalRunning(command: string | null): void {
    openTerminalPlaced((pane) => {
      return { type: 'terminalOpened', pane }
    }, command)
  }

  function openTerminal(): void {
    openTerminalRunning(null)
  }

  function openTerminalIn(tabGroup: TabGroupId | null): void {
    openTerminalPlaced((pane) => {
      return { type: 'terminalOpenedIn', pane, tabGroup }
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
    options: PtyCreateOptions,
    spawnedRequest: (pane: SessionId) => WorkspaceRequest
  ): void {
    void createSession(options).then((sessionId) => {
      const request = spawnedRequest(sessionId)
      void placeOrClose(sessionId, request)
    })
  }

  // The new shell starts in the split terminal's directory, which the main
  // process resolves from the pty it names here.
  function splitActive(): void {
    const workspace = link.getDrawn().workspace
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

    splitOnceSpawned({ cwdFrom: activeSessionId }, (pane) => {
      return { type: 'splitSpawned', donor: activeSessionId, pane }
    })
  }

  // The main process finds the agent session to fork from the pane named here.
  function fork(source: SessionId): void {
    const workspace = link.getDrawn().workspace
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

    splitOnceSpawned({ fork: { from: source, name } }, (pane) => {
      return { type: 'forkSpawned', source, pane }
    })
  }

  // Panes are created in group order and sent in that order: the workspace hands
  // the ids back out by walking the snapshot's groups the same way.
  async function restore(snapshot: Snapshot): Promise<void> {
    const sessionIds: SessionId[] = []
    let placed = false
    try {
      for (const group of snapshot.groups) {
        for (const pane of group.panes) {
          const startOptions = startOptionsOf(pane)
          const sessionId = await createSession(startOptions)
          sessionIds.push(sessionId)
        }
      }
      const reply = await link.request({ type: 'restored', panes: sessionIds })
      placed = reply.ok
    } catch {
      placed = false
    }

    if (!placed) {
      for (const sessionId of sessionIds) {
        disposeSession(sessionId)
      }
      openTerminal()
    }
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
    link.focus(pane)
    focusSession(pane)
  }

  function moveFocus(move: FocusMove): void {
    const workspace = link.getDrawn().workspace
    if (workspace === null) {
      return
    }

    const moved = focusAfter(workspace, move)
    const target = activeSessionOf(moved)
    if (target === null) {
      return
    }
    link.focus(target)
  }

  function colorPane(pane: SessionId, color: RowColor | null): void {
    send({ type: 'paneColored', pane, color })
  }

  function toggleActiveColor(color: RowColor): void {
    send({ type: 'activeColorToggled', color })
  }

  function drop(dragged: Dragged, target: Drop): void {
    send({ type: 'dropped', dragged, drop: target })
  }

  function resizeGroup(groupIndex: number, dividerIndex: number, leftWidth: number): void {
    send({ type: 'dividerMoved', groupIndex, dividerIndex, leftWidth })
  }

  async function newTabGroup(pane: SessionId): Promise<TabGroupId | null> {
    const reply = await link.request({ type: 'tabGroupCreated', pane })
    return reply.tabGroup
  }

  function renameTabGroup(id: TabGroupId, name: string): void {
    send({ type: 'tabGroupRenamed', id, name })
  }

  function recolorTabGroup(id: TabGroupId, color: RowColor): void {
    send({ type: 'tabGroupRecolored', id, color })
  }

  function ungroupTabGroup(id: TabGroupId): void {
    send({ type: 'tabGroupUngrouped', id })
  }

  function toggleTabGroup(id: TabGroupId): void {
    send({ type: 'tabGroupToggled', id })
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
    // typing `exit` and closing the terminal from the app take one path. The main
    // process takes the pane out on that exit; the window only lets go of its terminal.
    const stopExit = window.tacoShells.pty.onExit((event) => {
      disposeSession(event.id)
    })

    const stopTitle = onSessionTitle((sessionId, title) => {
      send({ type: 'titleChanged', pane: sessionId, title })
    })

    // Clicking straight into a pane has to move the active pane with it, or cmd+w
    // would close a terminal the user is not typing in. The focus effect in
    // useWorkspace refocuses on that change, but link.focus ignores a pane that is
    // already active, so this cannot loop.
    const stopFocus = onSessionFocus((sessionId) => {
      link.focus(sessionId)
    })

    return () => {
      stopExit()
      stopTitle()
      stopFocus()
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
