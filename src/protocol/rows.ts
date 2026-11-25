import type { SessionId } from './ipc'

export type RowState = 'working' | 'needs' | 'done' | 'idle'
// 'resume' is a start whose session already holds a conversation.
export type AgentSignal = 'start' | 'resume' | 'working' | 'needs' | 'stopped' | 'end'

export interface AgentRow {
  harness: string
  sessionId: string
  state: RowState
  hasConversation: boolean
}

export type AgentRows = ReadonlyMap<SessionId, AgentRow>
