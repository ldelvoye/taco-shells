import type { PtyExitEvent, PtySize, SessionId } from './ipc'
import type { TerminalStart, TerminalStarted } from './runtime'
import type { TerminalColors } from './theme'

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
  | { type: 'scrollback'; lines: number }
  | { type: 'colors'; colors: TerminalColors }
  | { type: 'ask'; ask: number; query: RuntimeQuery }

/** What the runtime sends the main process. */
export type RuntimeReport =
  | { type: 'started'; event: TerminalStarted }
  | { type: 'exited'; event: PtyExitEvent }
  | { type: 'titled'; id: SessionId; title: string }
  | { type: 'submitted'; id: SessionId }
  | { type: 'answer'; ask: number; value: string | string[] | null }

/** What a window asks of a terminal over its connection. `write` and `resize` work with no view attached. */
export type TerminalRequest =
  | { type: 'attach'; id: SessionId; size: PtySize | null }
  | { type: 'detach'; id: SessionId }
  | { type: 'write'; id: SessionId; data: string }
  | { type: 'resize'; id: SessionId; size: PtySize }
  | { type: 'clear'; id: SessionId }

/** What an attached view is sent, in the order it has to apply it. */
export type TerminalEvent =
  | { type: 'attached'; id: SessionId; size: PtySize; snapshot: string }
  | { type: 'output'; id: SessionId; data: string }
  | { type: 'resized'; id: SessionId; size: PtySize }
  | { type: 'cleared'; id: SessionId }
  | { type: 'end'; id: SessionId; exitCode: number }
