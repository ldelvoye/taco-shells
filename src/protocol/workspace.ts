import type { Dragged, Drop, RowColor, TabGroupId, Workspace } from './arrangement'
import type { ConfigFile } from './config'
import type { SessionId } from './ipc'
import type { AgentRow } from './rows'

export type FocusMove = 'nextPane' | 'previousPane' | 'nextGroup' | 'previousGroup'

/** What the user asked for. The workspace applies it or drops it. */
export type WorkspaceRequest =
  | { type: 'openTerminal'; file: ConfigFile | null }
  | { type: 'openTerminalIn'; tabGroup: TabGroupId | null }
  | { type: 'splitTerminal'; pane: SessionId }
  | { type: 'forkTerminal'; pane: SessionId }
  | { type: 'closeTerminal'; pane: SessionId }
  | { type: 'titleChanged'; pane: SessionId; title: string }
  | { type: 'focusReported'; pane: SessionId; count: number }
  | { type: 'paneColored'; pane: SessionId; color: RowColor | null }
  | { type: 'activeColorToggled'; color: RowColor }
  | { type: 'dropped'; dragged: Dragged; drop: Drop }
  | { type: 'dividerMoved'; groupIndex: number; dividerIndex: number; leftWidth: number }
  | { type: 'tabGroupCreated'; pane: SessionId }
  | { type: 'tabGroupRenamed'; id: TabGroupId; name: string }
  | { type: 'tabGroupRecolored'; id: TabGroupId; color: RowColor }
  | { type: 'tabGroupUngrouped'; id: TabGroupId }
  | { type: 'tabGroupToggled'; id: TabGroupId }

/**
 * `ok` is false when the request was not applied: a split with no room, a fork of a terminal that cannot fork.
 * `revision` is the state that holds the request's effect, which the window waits for before acting on the reply.
 */
export interface RequestReply {
  ok: boolean
  tabGroup: TabGroupId | null
  revision: number
}

/** What the window needs to know about one agent: its name in a menu, and whether its sessions can be forked. */
export interface AgentLabel {
  name: string
  forks: boolean
}

/** Everything the window draws. */
export interface DrawnState {
  revision: number
  workspace: Workspace | null
  agentRows: Record<SessionId, AgentRow>
  waiting: SessionId | null
  /** A counter that goes up each time the window should hand the keyboard back to the active terminal. */
  refocus: number
  /** How many of the window's own focus moves the workspace has applied: the highest `count` it was sent. */
  focusCount: number
  agents: Record<string, AgentLabel>
  defaultAgentName: string
  problem: string | null
}
