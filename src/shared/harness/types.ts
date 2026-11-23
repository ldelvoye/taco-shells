import type { AgentSignal } from '../rowState'

export interface AgentTitle {
  name: string
  idle: boolean
}

/** What the app knows about one coding agent. */
export interface Harness {
  /** The agent's name on the wire and in the saved workspace. */
  id: string
  /** The agent's name in the menu: "Fork with <name>". */
  name: string
  /** Whether a value is one of this agent's session ids. An id is typed into a shell, so nothing else may pass. */
  isSessionId(value: unknown): value is string
  /** The signal a hook event stands for, or null for an event that changes nothing. */
  signalFor(event: string, detail: string | null): AgentSignal | null
  /** A terminal title as a row name and an idle flag. Without it, rows show the raw title. */
  titleOf?(title: string): AgentTitle
  /** The line typed into a shell to resume a session, or null to type nothing. */
  resumeCommand(sessionId: string): string | null
  /** The line typed into a shell to fork a session under a name. Without it, rows never offer Fork. */
  forkCommand?(sessionId: string, name: string): string | null
}
