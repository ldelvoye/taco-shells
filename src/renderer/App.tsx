import { type JSX, useCallback, useEffect, useMemo, useState } from 'react'
import type { SessionId } from '@shared/ipc'
import {
  activePane,
  activateGroup,
  addGroup,
  closePane,
  emptyWorkspace,
  type Group,
  moveGroup,
  renamePane,
  type Workspace
} from '@shared/model'
import { createCommandRegistry } from './commands/registry'
import { registerTerminalCommands } from './commands/terminal'
import { registerWorkspaceCommands } from './commands/workspace'
import { keyBindings } from './keys/bindings'
import { useCommandKeys } from './keys/useCommandKeys'
import { Sidebar } from './sidebar/Sidebar'
import { createSession, disposeSession, focusSession, onSessionTitle } from './terminal/registry'
import { TerminalView } from './terminal/TerminalView'

let nextGroupNumber = 1

function groupForSession(sessionId: SessionId): Group {
  const id = `group-${nextGroupNumber}`
  nextGroupNumber += 1
  return { id, panes: [{ id: sessionId, title: '' }], activePane: 0 }
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

  const openTerminal = useCallback(() => {
    void createSession().then((sessionId) => {
      const group = groupForSession(sessionId)
      setWorkspace((current) => {
        let base = current
        if (!base) {
          base = emptyWorkspace()
        }
        return addGroup(base, group)
      })
    })
  }, [])

  useEffect(() => {
    openTerminal()
  }, [openTerminal])

  useEffect(() => {
    return onSessionTitle((sessionId, title) => {
      updateWorkspace((current) => renamePane(current, sessionId, title))
    })
  }, [updateWorkspace])

  // A pane leaves the workspace when its pty exits and at no other time, so
  // typing `exit` and closing the terminal from the app take one path.
  useEffect(() => {
    return window.taqueria.pty.onExit((event) => {
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

  const toggleSidebar = useCallback(() => {
    setSidebarVisible((visible) => {
      return !visible
    })
  }, [])

  // Focused here rather than left to the effect above, because clicking the row
  // that is already active changes no state and so would fire no effect, while
  // the click itself has already taken focus off the terminal.
  const selectGroup = useCallback(
    (index: number, sessionId: SessionId) => {
      updateWorkspace((current) => activateGroup(current, index))
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

  useEffect(() => {
    registerWorkspaceCommands(commands, { openTerminal, closeActiveTerminal, toggleSidebar })
  }, [commands, openTerminal, closeActiveTerminal, toggleSidebar])

  useEffect(() => {
    if (!activeSessionId) {
      return
    }
    registerTerminalCommands(commands, activeSessionId)
  }, [commands, activeSessionId])

  useCommandKeys(commands, keyBindings)

  let groups: Group[] = []
  let activeGroupIndex = 0
  if (workspace) {
    groups = workspace.groups
    activeGroupIndex = workspace.activeGroup
  }

  const panes = groups.flatMap((group) => group.panes)
  const terminals = panes.map((pane) => {
    return <TerminalView key={pane.id} sessionId={pane.id} active={pane.id === activeSessionId} />
  })

  let sidebar = null
  if (sidebarVisible) {
    sidebar = (
      <Sidebar
        groups={groups}
        activeGroup={activeGroupIndex}
        onSelect={selectGroup}
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
        <main className="pane-area">{terminals}</main>
      </div>
    </div>
  )
}
