import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'
import type { Workspace } from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'
import type { AgentRows } from '@protocol/rows'
import type { AgentLabel } from '@protocol/workspace'
import { activeSessionOf } from '@rules/arrangement'
import { disposeSessionsNotIn, focusSession } from '../terminal/registry'
import { createWorkspaceLink, type WorkspaceLink } from './link'
import { createWorkspaceOperations, type WorkspaceOperations } from './operations'

export interface WorkspaceViewState {
  workspace: Workspace | null
  agentRows: AgentRows
  waiting: SessionId | null
  agents: Record<string, AgentLabel>
  defaultAgentName: string
  problem: string | null
}

export interface WorkspaceView {
  state: WorkspaceViewState
  operations: WorkspaceOperations
}

interface WorkspaceParts {
  link: WorkspaceLink
  operations: WorkspaceOperations
}

function createWorkspaceParts(): WorkspaceParts {
  const link = createWorkspaceLink(window.tacoShells.workspace)
  const operations = createWorkspaceOperations(link)
  return { link, operations }
}

export function useWorkspace(): WorkspaceView {
  const [parts] = useState(createWorkspaceParts)
  const { link, operations } = parts
  const drawn = useSyncExternalStore(link.subscribe, link.getDrawn)
  const workspace = drawn.workspace
  const activeSessionId = activeSessionOf(workspace)
  const refocus = drawn.refocus

  const rows = drawn.agentRows
  const agentRows = useMemo(() => {
    const entries = Object.entries(rows)
    return new Map(entries)
  }, [rows])

  useEffect(() => {
    return operations.listen()
  }, [operations])

  useEffect(() => {
    const open = new Set<SessionId>()
    if (workspace !== null) {
      for (const group of workspace.groups) {
        for (const pane of group.panes) {
          open.add(pane.id)
        }
      }
    }
    disposeSessionsNotIn(open)
  }, [workspace])

  useEffect(() => {
    if (!activeSessionId) {
      return
    }
    focusSession(activeSessionId)
  }, [activeSessionId])

  // The keyboard can have to go back to the active terminal without the active one changing:
  // a sidebar action took it away, or the app or a hook asked for the terminal that already has it.
  useEffect(() => {
    if (refocus === 0) {
      return
    }
    const latest = link.getDrawn().workspace
    const active = activeSessionOf(latest)
    if (!active) {
      return
    }
    focusSession(active)
  }, [link, refocus])

  const state: WorkspaceViewState = {
    workspace,
    agentRows,
    waiting: drawn.waiting,
    agents: drawn.agents,
    defaultAgentName: drawn.defaultAgentName,
    problem: drawn.problem
  }
  return { state, operations }
}
