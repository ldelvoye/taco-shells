import type { PtyExitEvent, PtySize, SessionId } from './ipc'
import type { TerminalColors } from './theme'

/** A terminal to open: the workspace has already named it and placed it. */
export interface TerminalStart {
  id: SessionId
  /** The directory to start in. When null, `directoryOf` is tried, then the home directory. */
  directory: string | null
  /** A terminal whose current directory to start in, looked up when the shell is spawned. */
  directoryOf: SessionId | null
  /** Typed into the shell once it has started, or null. */
  command: string | null
  size: PtySize
}

export interface TerminalStarted {
  id: SessionId
  directory: string
}

/** What the workspace and the services ask of the terminal runtime. It knows no arrangement, no window and no agent. */
export interface Runtime {
  /** Returns at once. A shell that cannot start reports an exit like any other. */
  open(start: TerminalStart): void
  close(id: SessionId): void
  write(id: SessionId, data: string): void
  resize(id: SessionId, size: PtySize): void
  setScrollback(lines: number): void
  setColors(colors: TerminalColors): void
  onStarted(listener: (event: TerminalStarted) => void): void
  onExit(listener: (event: PtyExitEvent) => void): void
  onTitle(listener: (id: SessionId, title: string) => void): void
  onSubmit(listener: (id: SessionId) => void): void
  directoryOf(id: SessionId): Promise<string | null>
  foregroundCommands(id: SessionId): Promise<string[]>
  terminalForPids(pids: number[]): Promise<SessionId | null>
}
