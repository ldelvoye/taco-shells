import type { Dragged, Drop, RowColor, TabGroupId } from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'
import type { FocusMove, WorkspaceRequest } from '@protocol/workspace'
import { activeSessionOf, canSplitPane } from '@rules/arrangement'
import { focusAfter } from '@rules/focus'
import {
  disposeSession,
  focusSession,
  onSessionFocus,
  onSessionTitle
} from '../terminal/registry'
import type { WorkspaceLink } from './link'

export interface WorkspaceOperations {
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

export function createWorkspaceOperations(link: WorkspaceLink): WorkspaceOperations {
  // Nothing waits on these, so a request that fails is reported here or nowhere.
  function send(request: WorkspaceRequest): void {
    link.request(request).catch((failure: unknown) => {
      console.error('a workspace request failed', failure)
    })
  }

  function activeSession(): SessionId | null {
    const workspace = link.getDrawn().workspace
    return activeSessionOf(workspace)
  }

  function openTerminal(): void {
    send({ type: 'openTerminal', file: null })
  }

  function openTerminalIn(tabGroup: TabGroupId | null): void {
    send({ type: 'openTerminalIn', tabGroup })
  }

  function openSettings(): void {
    send({ type: 'openTerminal', file: 'settings' })
  }

  function openKeybindings(): void {
    send({ type: 'openTerminal', file: 'keybindings' })
  }

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

    send({ type: 'splitTerminal', pane: activeSessionId })
  }

  function fork(source: SessionId): void {
    const workspace = link.getDrawn().workspace
    if (!workspace) {
      return
    }
    if (!canSplitPane(workspace, source)) {
      return
    }

    send({ type: 'forkTerminal', pane: source })
  }

  function closeActive(): void {
    const activeSessionId = activeSession()
    if (!activeSessionId) {
      return
    }
    send({ type: 'closeTerminal', pane: activeSessionId })
  }

  function close(pane: SessionId): void {
    send({ type: 'closeTerminal', pane })
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
    // A pane leaves the workspace when its pty exits or the runtime process dies, so
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
