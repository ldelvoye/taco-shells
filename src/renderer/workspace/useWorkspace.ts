import { useEffect, useState, useSyncExternalStore } from 'react'
import { activeSessionOf, type WorkspaceState } from '@shared/workspaceState'
import { focusSession } from '../terminal/registry'
import { createWorkspaceOperations, type WorkspaceOperations } from './operations'
import { createWorkspaceStore, type WorkspaceStore } from './store'

export interface WorkspaceView {
  state: WorkspaceState
  operations: WorkspaceOperations
}

interface WorkspaceParts {
  store: WorkspaceStore
  operations: WorkspaceOperations
}

function createWorkspaceParts(): WorkspaceParts {
  const store = createWorkspaceStore()
  const operations = createWorkspaceOperations(store)
  return { store, operations }
}

export function useWorkspace(): WorkspaceView {
  const [parts] = useState(createWorkspaceParts)
  const { store, operations } = parts
  const state = useSyncExternalStore(store.subscribe, store.getDrawnState)
  const workspace = state.workspace
  const activeSessionId = activeSessionOf(workspace)
  const refocus = state.refocus

  useEffect(() => {
    return operations.listen()
  }, [operations])

  useEffect(() => {
    operations.start()
  }, [operations])

  useEffect(() => {
    if (!workspace) {
      return
    }
    window.tacoShells.snapshot.workspaceChanged(workspace)
  }, [workspace])

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

  // A sidebar action can take the keyboard off the active terminal without changing which one is active.
  useEffect(() => {
    if (refocus === 0) {
      return
    }
    const latest = store.getState().workspace
    const active = activeSessionOf(latest)
    if (!active) {
      return
    }
    focusSession(active)
  }, [store, refocus])

  return { state, operations }
}
