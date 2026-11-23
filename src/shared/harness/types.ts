import type { ClaudeSignal } from '../rowState'

export interface AgentTitle {
  name: string
  idle: boolean
}

export interface Harness {
  id: string
  name: string
  isSessionId(value: unknown): value is string
  signalFor(event: string, detail: string | null): ClaudeSignal | null
  titleOf?(title: string): AgentTitle
  resumeCommand(sessionId: string): string | null
  forkCommand?(sessionId: string, name: string): string | null
}
