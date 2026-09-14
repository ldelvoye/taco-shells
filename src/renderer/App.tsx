import { type JSX, useCallback, useEffect, useMemo, useState } from 'react'
import type { SessionId } from '@shared/ipc'
import { paletteFor } from '@shared/theme'
import {
  activatePane,
  activePane,
  addGroup,
  canSplitActivePane,
  closePane,
  emptyWorkspace,
  focusNextGroup,
  focusNextPane,
  focusPreviousGroup,
  focusPreviousPane,
  type Group,
  moveDivider,
  moveGroup,
  renamePane,
  splitActivePane,
  type Workspace
} from '@shared/model'
import { createCommandRegistry } from './commands/registry'
import { registerTerminalCommands } from './commands/terminal'
import { registerWorkspaceCommands } from './commands/workspace'
import { ConfigBanner } from './config/ConfigBanner'
import { applyChromePalette } from './config/palette'
import { useConfig } from './config/useConfig'
import { useCommandKeys } from './keys/useCommandKeys'
import { PaneArea } from './layout/PaneArea'
import { Sidebar } from './sidebar/Sidebar'
import {
  applyTerminalConfig,
  createSession,
  disposeSession,
  focusSession,
  onSessionFocus,
  onSessionTitle
} from './terminal/registry'

let nextGroupNumber = 1

function groupForSession(sessionId: SessionId): Group {
  const id = `group-${nextGroupNumber}`
  nextGroupNumber += 1
  return { id, panes: [{ id: sessionId, title: '', width: 1 }], activePane: 0 }
}

// The shell resolves EDITOR, not us: on a GUI launch the app's own environment
// does not have it, because it is set in the files an interactive shell reads.
function editorCommand(path: string): string {
  return `\${EDITOR:-vi} '${path}'\n`
}

function activeSessionOf(workspace: Workspace | null): SessionId | null {
  if (!workspace) {
    return null
  }

  const pane = activePane(workspace)
  if (!pane) {
    return null
  }

  return pane.id
}

export function App(): JSX.Element {
  // Null is the app before its first terminal exists, which is not the same
  // state as a workspace whose last terminal has just closed.
  const [workspace, setWorkspace] = useState<Workspace | null>(null)
  const [sidebarVisible, setSidebarVisible] = useState(true)
  const commands = useMemo(() => createCommandRegistry(), [])
  const activeSessionId = activeSessionOf(workspace)
  const config = useConfig()
  const bindings = useMemo(() => new Map(config.bindings), [config.bindings])

  // Changes before the first terminal exists are dropped: there is nothing yet
  // for them to describe.
  const updateWorkspace = useCallback((change: (workspace: Workspace) => Workspace) => {
    setWorkspace((current) => {
      if (!current) {
        return current
      }
      return change(current)
    })
  }, [])

  const openTerminalRunning = useCallback((command: string | null) => {
    void createSession().then((sessionId) => {
      const group = groupForSession(sessionId)
      setWorkspace((current) => {
        let base = current
        if (!base) {
          base = emptyWorkspace()
        }
        return addGroup(base, group)
      })
      if (command !== null) {
        window.tacoShells.pty.write(sessionId, command)
      }
    })
  }, [])

  const openTerminal = useCallback(() => {
    openTerminalRunning(null)
  }, [openTerminalRunning])

  const openSettings = useCallback(() => {
    const path = window.tacoShells.config.pathOf('settings')
    const command = editorCommand(path)
    openTerminalRunning(command)
  }, [openTerminalRunning])

  const openKeybindings = useCallback(() => {
    const path = window.tacoShells.config.pathOf('keybindings')
    const command = editorCommand(path)
    openTerminalRunning(command)
  }, [openTerminalRunning])

  // The new shell starts in the split terminal's directory, which the main
  // process resolves from the pty it names here.
  const splitTerminal = useCallback(() => {
    if (!activeSessionId) {
      return
    }
    if (!workspace) {
      return
    }
    // Asked before the pty is spawned: a split the model would refuse must not
    // leave a shell running with no pane to show it in.
    if (!canSplitActivePane(workspace)) {
      return
    }

    void createSession(activeSessionId).then((sessionId) => {
      updateWorkspace((current) => splitActivePane(current, sessionId))
    })
  }, [activeSessionId, workspace, updateWorkspace])

  useEffect(() => {
    openTerminal()
  }, [openTerminal])

  useEffect(() => {
    return onSessionTitle((sessionId, title) => {
      updateWorkspace((current) => renamePane(current, sessionId, title))
    })
  }, [updateWorkspace])

  // Clicking straight into a pane has to move the workspace's idea of the active
  // pane with it, or cmd+w would close a terminal the user is not typing in. The
  // effect below refocuses on that change, but activatePane returns the
  // workspace untouched once the pane is already active, so this cannot loop.
  useEffect(() => {
    return onSessionFocus((sessionId) => {
      updateWorkspace((current) => activatePane(current, sessionId))
    })
  }, [updateWorkspace])

  // A pane leaves the workspace when its pty exits and at no other time, so
  // typing `exit` and closing the terminal from the app take one path.
  useEffect(() => {
    return window.tacoShells.pty.onExit((event) => {
      disposeSession(event.id)
      updateWorkspace((current) => closePane(current, event.id))
    })
  }, [updateWorkspace])

  // The terminals are the window's only contents, so losing the last of them is
  // the same gesture as closing the window.
  useEffect(() => {
    if (!workspace) {
      return
    }
    if (workspace.groups.length > 0) {
      return
    }
    window.close()
  }, [workspace])

  useEffect(() => {
    if (!activeSessionId) {
      return
    }
    focusSession(activeSessionId)
  }, [activeSessionId])

  const closeActiveTerminal = useCallback(() => {
    if (!activeSessionId) {
      return
    }
    disposeSession(activeSessionId)
  }, [activeSessionId])

  const focusNext = useCallback(() => {
    updateWorkspace((current) => focusNextPane(current))
  }, [updateWorkspace])

  const focusPrevious = useCallback(() => {
    updateWorkspace((current) => focusPreviousPane(current))
  }, [updateWorkspace])

  const focusNextTerminal = useCallback(() => {
    updateWorkspace((current) => focusNextGroup(current))
  }, [updateWorkspace])

  const focusPreviousTerminal = useCallback(() => {
    updateWorkspace((current) => focusPreviousGroup(current))
  }, [updateWorkspace])

  const toggleSidebar = useCallback(() => {
    setSidebarVisible((visible) => {
      return !visible
    })
  }, [])

  // Focused here rather than left to the effect above, because clicking the row
  // of the pane that is already active changes no state and so would fire no
  // effect, while the click itself has already taken focus off the terminal.
  const selectPane = useCallback(
    (sessionId: SessionId) => {
      updateWorkspace((current) => activatePane(current, sessionId))
      focusSession(sessionId)
    },
    [updateWorkspace]
  )

  const reorderGroups = useCallback(
    (from: number, to: number) => {
      updateWorkspace((current) => moveGroup(current, from, to))
    },
    [updateWorkspace]
  )

  const resizeGroup = useCallback(
    (groupIndex: number, dividerIndex: number, leftWidth: number) => {
      updateWorkspace((current) => moveDivider(current, groupIndex, dividerIndex, leftWidth))
    },
    [updateWorkspace]
  )

  useEffect(() => {
    registerWorkspaceCommands(commands, {
      openTerminal,
      splitTerminal,
      closeActiveTerminal,
      focusNextPane: focusNext,
      focusPreviousPane: focusPrevious,
      focusNextGroup: focusNextTerminal,
      focusPreviousGroup: focusPreviousTerminal,
      toggleSidebar,
      openSettings,
      openKeybindings
    })
  }, [
    commands,
    openTerminal,
    splitTerminal,
    closeActiveTerminal,
    focusNext,
    focusPrevious,
    focusNextTerminal,
    focusPreviousTerminal,
    toggleSidebar,
    openSettings,
    openKeybindings
  ])

  useEffect(() => {
    if (!activeSessionId) {
      return
    }
    registerTerminalCommands(commands, activeSessionId)
  }, [commands, activeSessionId])

  useEffect(() => {
    const palette = paletteFor(config.appearance)
    applyChromePalette(palette)
  }, [config.appearance])

  useEffect(() => {
    const palette = paletteFor(config.appearance)
    applyTerminalConfig(config.settings, palette.terminal)
  }, [config])

  useCommandKeys(commands, bindings)

  let groups: Group[] = []
  let activeGroupIndex = 0
  if (workspace) {
    groups = workspace.groups
    activeGroupIndex = workspace.activeGroup
  }

  let sidebar = null
  if (sidebarVisible) {
    sidebar = (
      <Sidebar
        groups={groups}
        activeGroup={activeGroupIndex}
        onSelect={selectPane}
        onReorder={reorderGroups}
      />
    )
  }

  // Hiding the sidebar takes the traffic lights' strip with it, so the pane area
  // grows its own. It is the only place a frameless window can be dragged by.
  let paneTitlebar = null
  if (!sidebarVisible) {
    paneTitlebar = <div className="pane-titlebar" />
  }

  return (
    <div className="app">
      {sidebar}
      <div className="pane-column">
        {paneTitlebar}
        <ConfigBanner problems={config.problems} />
        <PaneArea groups={groups} activeGroup={activeGroupIndex} onResize={resizeGroup} />
      </div>
    </div>
  )
}
