import { statSync, type Stats } from 'node:fs'
import { homedir } from 'node:os'
import { spawn, type IPty } from 'node-pty'
import type { PtyDataEvent, PtyExitEvent, PtySize, SessionId } from '@protocol/ipc'
import type { Runtime, TerminalStart, TerminalStarted } from '@protocol/runtime'
import { processCwd } from './cwd'
import { foregroundCommands } from './foreground'
import { shellArguments, shellEnvironment, shellPath } from './shell'

export type SpawnedPty = Pick<IPty, 'pid' | 'write' | 'resize' | 'kill' | 'onData' | 'onExit'>
export type SpawnPty = (...args: Parameters<typeof spawn>) => SpawnedPty

type DataListener = (event: PtyDataEvent) => void
type ExitListener = (event: PtyExitEvent) => void
type StartedListener = (event: TerminalStarted) => void
type SubmitListener = (id: SessionId) => void

interface Session {
  pty: SpawnedPty
  pending: string[]
  flushTimer: ReturnType<typeof setTimeout> | null
}

// A terminal is on screen before its shell exists: finding the starting directory can take
// up to a second. What is asked of it in that gap is kept and applied once the shell is there.
interface Opening {
  size: PtySize
  closed: boolean
  typed: string[]
}

// A pty in line-buffered mode emits roughly one chunk per line: `seq 1 200000`
// arrives as ~78,000 chunks of about 19 bytes. Emitting each one separately
// means an IPC message and an xterm write each, and past about a million of
// those the renderer stops keeping up and its memory climbs without bound.
// Coalescing on a short timer turns a burst into a few hundred messages a
// second, and 5ms is well inside a frame, so an echoed keystroke still looks
// immediate.
const FLUSH_INTERVAL_MS = 5
const CARRIAGE_RETURN = '\r'

export class PtySessions implements Runtime {
  private readonly sessions = new Map<SessionId, Session>()
  private readonly opening = new Map<SessionId, Opening>()
  private readonly dataListeners = new Set<DataListener>()
  private readonly exitListeners = new Set<ExitListener>()
  private readonly startedListeners = new Set<StartedListener>()
  private readonly submitListeners = new Set<SubmitListener>()
  private readonly spawnPty: SpawnPty

  constructor(spawnPty: SpawnPty = spawn) {
    this.spawnPty = spawnPty
  }

  open(start: TerminalStart): void {
    this.opening.set(start.id, { size: start.size, closed: false, typed: [] })
    void this.spawnWhenReady(start)
  }

  private async spawnWhenReady(start: TerminalStart): Promise<void> {
    const id = start.id
    const cwd = await this.resolveStartingDirectory(start)

    const opening = this.opening.get(id)
    this.opening.delete(id)
    if (opening === undefined || opening.closed) {
      this.reportExit({ id, exitCode: 0 })
      return
    }

    let pty: SpawnedPty
    try {
      pty = this.spawnPty(shellPath(), shellArguments(), {
        name: 'xterm-256color',
        cols: opening.size.cols,
        rows: opening.size.rows,
        cwd,
        env: shellEnvironment()
      })
    } catch {
      this.reportExit({ id, exitCode: 1 })
      return
    }

    const session: Session = { pty, pending: [], flushTimer: null }
    this.sessions.set(id, session)

    if (start.command !== null) {
      pty.write(start.command)
    }
    for (const data of opening.typed) {
      this.write(id, data)
    }

    pty.onData((data) => {
      session.pending.push(data)
      if (session.flushTimer) {
        return
      }
      session.flushTimer = setTimeout(() => {
        this.flush(id)
      }, FLUSH_INTERVAL_MS)
    })

    pty.onExit(({ exitCode }) => {
      this.flush(id)
      this.clearFlushTimer(session)
      this.sessions.delete(id)
      this.reportExit({ id, exitCode })
    })

    for (const listener of this.startedListeners) {
      listener({ id, directory: cwd })
    }
  }

  write(id: SessionId, data: string): void {
    const opening = this.opening.get(id)
    if (opening !== undefined) {
      opening.typed.push(data)
      return
    }

    const session = this.sessions.get(id)
    if (!session) {
      return
    }
    session.pty.write(data)
    if (data.includes(CARRIAGE_RETURN)) {
      for (const listener of this.submitListeners) {
        listener(id)
      }
    }
  }

  resize(id: SessionId, size: PtySize): void {
    const opening = this.opening.get(id)
    if (opening !== undefined) {
      opening.size = size
      return
    }

    const session = this.sessions.get(id)
    if (!session) {
      return
    }
    // node-pty throws on a pty whose shell has just exited, which a resize can reach before the exit event does.
    try {
      session.pty.resize(size.cols, size.rows)
    } catch {
      return
    }
  }

  close(id: SessionId): void {
    const opening = this.opening.get(id)
    if (opening !== undefined) {
      opening.closed = true
      return
    }

    const session = this.sessions.get(id)
    if (!session) {
      return
    }

    this.flush(id)
    this.clearFlushTimer(session)
    this.sessions.delete(id)
    session.pty.kill()
  }

  onData(listener: DataListener): void {
    this.dataListeners.add(listener)
  }

  onExit(listener: ExitListener): void {
    this.exitListeners.add(listener)
  }

  /** Called when a write holds a carriage return: a submitted line, or pasted text with one in it. */
  onSubmit(listener: SubmitListener): void {
    this.submitListeners.add(listener)
  }

  onStarted(listener: StartedListener): void {
    this.startedListeners.add(listener)
  }

  async directoryOf(id: SessionId): Promise<string | null> {
    const session = this.sessions.get(id)
    if (session === undefined) {
      return null
    }
    return processCwd(session.pty.pid)
  }

  async foregroundCommands(id: SessionId): Promise<string[]> {
    const session = this.sessions.get(id)
    if (session === undefined) {
      return []
    }
    return foregroundCommands(session.pty.pid)
  }

  terminalForPids(pids: number[]): Promise<SessionId | null> {
    let found: SessionId | null = null
    for (const [id, session] of this.sessions) {
      const pid = session.pty.pid
      if (pids.includes(pid)) {
        found = id
        break
      }
    }
    return Promise.resolve(found)
  }

  private reportExit(event: PtyExitEvent): void {
    for (const listener of this.exitListeners) {
      listener(event)
    }
  }

  private async resolveStartingDirectory(start: TerminalStart): Promise<string> {
    let candidate: string | null = null
    if (start.directory !== null) {
      candidate = start.directory
    } else if (start.directoryOf !== null) {
      candidate = await this.directoryOf(start.directoryOf)
    }
    return this.usableDirectory(candidate)
  }

  private usableDirectory(candidate: string | null): string {
    if (candidate === null) {
      return homedir()
    }

    let stats: Stats
    try {
      stats = statSync(candidate)
    } catch {
      return homedir()
    }

    if (!stats.isDirectory()) {
      return homedir()
    }

    return candidate
  }

  private flush(id: SessionId): void {
    const session = this.sessions.get(id)
    if (!session) {
      return
    }

    session.flushTimer = null
    if (session.pending.length === 0) {
      return
    }

    const data = session.pending.join('')
    session.pending = []
    for (const listener of this.dataListeners) {
      listener({ id, data })
    }
  }

  private clearFlushTimer(session: Session): void {
    if (!session.flushTimer) {
      return
    }
    clearTimeout(session.flushTimer)
    session.flushTimer = null
  }
}
