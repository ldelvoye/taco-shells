import type { PtyDataEvent, PtyExitEvent, PtySize, SessionId } from './ipc'
import type { TerminalStart, TerminalStarted } from './runtime'

export type RuntimeQuery =
  | { kind: 'directoryOf'; id: SessionId }
  | { kind: 'foregroundCommands'; id: SessionId }
  | { kind: 'terminalForPids'; pids: number[] }

/** What the main process sends the runtime. */
export type RuntimeCommand =
  | { type: 'open'; start: TerminalStart }
  | { type: 'close'; id: SessionId }
  | { type: 'write'; id: SessionId; data: string }
  | { type: 'resize'; id: SessionId; size: PtySize }
  | { type: 'ask'; ask: number; query: RuntimeQuery }

/** What the runtime sends the main process. */
export type RuntimeReport =
  | { type: 'started'; event: TerminalStarted }
  | { type: 'exited'; event: PtyExitEvent }
  | { type: 'submitted'; id: SessionId }
  | { type: 'answer'; ask: number; value: string | string[] | null }

/** What the window sends the runtime on its own connection. */
export type TerminalInput =
  | { type: 'write'; id: SessionId; data: string }
  | { type: 'resize'; id: SessionId; size: PtySize }
  | { type: 'listening' }

/** What the runtime sends the window. An exit follows the last output of its terminal. */
export type TerminalOutput =
  | { type: 'data'; event: PtyDataEvent }
  | { type: 'exit'; event: PtyExitEvent }
