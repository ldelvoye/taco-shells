import { homedir } from 'node:os'
import { spawn, type IPty } from 'node-pty'
import type { PtyDataEvent, PtyExitEvent, PtySize, SessionId } from '@shared/ipc'
import { shellArguments, shellEnvironment, shellPath } from './shell'

type DataListener = (event: PtyDataEvent) => void
type ExitListener = (event: PtyExitEvent) => void

interface Session {
  pty: IPty
  pending: string[]
  flushTimer: ReturnType<typeof setTimeout> | null
}

// A pty in line-buffered mode emits roughly one chunk per line: `seq 1 200000`
// arrives as ~78,000 chunks of about 19 bytes. Emitting each one separately
// means an IPC message and an xterm write each, and past about a million of
// those the renderer stops keeping up and its memory climbs without bound.
// Coalescing on a short timer turns a burst into a few hundred messages a
// second, and 5ms is well inside a frame, so an echoed keystroke still looks
// immediate.
const FLUSH_INTERVAL_MS = 5

export class PtySessions {
  private readonly sessions = new Map<SessionId, Session>()
  private readonly dataListeners = new Set<DataListener>()
  private readonly exitListeners = new Set<ExitListener>()
  private nextId = 1

  create(size: PtySize): SessionId {
    const id = `pty-${this.nextId}`
    this.nextId += 1

    const pty = spawn(shellPath(), shellArguments(), {
      name: 'xterm-256color',
      cols: size.cols,
      rows: size.rows,
      cwd: homedir(),
      env: shellEnvironment()
    })

    const session: Session = { pty, pending: [], flushTimer: null }
    this.sessions.set(id, session)

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
      for (const listener of this.exitListeners) {
        listener({ id, exitCode })
      }
    })

    return id
  }

  write(id: SessionId, data: string): void {
    const session = this.sessions.get(id)
    if (!session) {
      return
    }
    session.pty.write(data)
  }

  resize(id: SessionId, size: PtySize): void {
    const session = this.sessions.get(id)
    if (!session) {
      return
    }
    session.pty.resize(size.cols, size.rows)
  }

  kill(id: SessionId): void {
    const session = this.sessions.get(id)
    if (!session) {
      return
    }

    this.flush(id)
    this.clearFlushTimer(session)
    this.sessions.delete(id)
    session.pty.kill()
  }

  killAll(): void {
    const ids = [...this.sessions.keys()]
    for (const id of ids) {
      this.kill(id)
    }
  }

  onData(listener: DataListener): void {
    this.dataListeners.add(listener)
  }

  onExit(listener: ExitListener): void {
    this.exitListeners.add(listener)
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
