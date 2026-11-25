import type { Dragged, Drop, RowColor, TabGroupId, Workspace } from './arrangement'
import type { SessionId } from './ipc'
import type { AgentRow } from './rows'

export type FocusMove = 'nextPane' | 'previousPane' | 'nextGroup' | 'previousGroup'

/** What the user asked for. The workspace applies it or drops it. */
export type WorkspaceRequest =
  | { type: 'terminalOpened'; pane: SessionId }
  | { type: 'terminalOpenedIn'; pane: SessionId; tabGroup: TabGroupId | null }
  | { type: 'restored'; panes: SessionId[] }
  | { type: 'splitSpawned'; donor: SessionId; pane: SessionId }
  | { type: 'forkSpawned'; source: SessionId; pane: SessionId }
  | { type: 'titleChanged'; pane: SessionId; title: string }
  | { type: 'focusReported'; pane: SessionId; move: number }
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
 * `ok` is false when a reported terminal could not be placed: the window still holds that shell and closes it.
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
  refocus: number
  /** The latest of the window's focus moves the workspace has applied. */
  focusMove: number
  agents: Record<string, AgentLabel>
  defaultAgentName: string
}
