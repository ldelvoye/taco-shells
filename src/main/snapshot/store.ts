import type { SessionId } from '@shared/ipc'
import type { Workspace } from '@shared/model'
import { type ClaudeSession, type Snapshot, snapshotFrom } from '@shared/snapshot'
import { processCwd } from '../pty/cwd'
import type { PtySessions } from '../pty/sessions'
import { readSnapshot, writeSnapshot } from './files'

export const SNAPSHOT_DEBOUNCE_MS = 250
export const CWD_REFRESH_MS = 30_000
export const QUIT_HOLD_MS = 1000
export const PROVISIONAL_TTL_MS = 60_000

interface ClaudeEntry {
  session: ClaudeSession
  provisionalSince: number | null
}

type SessionSource = Pick<PtySessions, 'pidOf' | 'onExit'>

export class SnapshotStore {
  private readonly directory: string
  private readonly sessions: SessionSource
  private readonly now: () => number
  private readonly initialSnapshot: Snapshot | null
  private readonly cwds = new Map<SessionId, string>()
  private readonly claude = new Map<SessionId, ClaudeEntry[]>()
  private readonly refreshTimer: ReturnType<typeof setInterval>
  private debounceTimer: ReturnType<typeof setTimeout> | null = null
  private latestWorkspace: Workspace | null = null
  private frozen = false

  constructor(directory: string, sessions: SessionSource, now: () => number = Date.now) {
    this.directory = directory
    this.sessions = sessions
    this.now = now
    this.initialSnapshot = readSnapshot(directory)

    sessions.onExit(({ id }) => {
      if (this.frozen) {
        return
      }
      this.cwds.delete(id)
      this.claude.delete(id)
    })

    this.refreshTimer = setInterval(() => {
      this.refreshDirectories()
        .then(() => {
          this.scheduleWrite()
        })
        .catch(() => undefined)
    }, CWD_REFRESH_MS)
  }

  /** The snapshot read when the store was created. */
  initial(): Snapshot | null {
    return this.initialSnapshot
  }

  /** The renderer's latest workspace, written after the debounce. */
  workspaceChanged(workspace: Workspace): void {
    if (this.frozen) {
      return
    }
    this.latestWorkspace = workspace
    this.scheduleWrite()
  }

  /** A Claude Code session that started in a pane; the newest one is what the pane resumes. */
  recordClaudeStart(id: SessionId, session: ClaudeSession): void {
    if (this.frozen) {
      return
    }

    const entries = this.entriesFor(id)
    const existing = entries.find((entry) => entry.session.sessionId === session.sessionId)
    if (existing === undefined) {
      entries.push({ session, provisionalSince: null })
    } else {
      existing.provisionalSince = null
    }
    this.scheduleWrite()
  }

  /** A Claude Code session that ended in a pane, dropped from what the pane resumes. */
  recordClaudeEnd(id: SessionId, sessionId: string): void {
    if (this.frozen) {
      return
    }

    const entries = this.claude.get(id)
    if (entries === undefined) {
      return
    }

    const remaining = entries.filter((entry) => entry.session.sessionId !== sessionId)
    if (remaining.length === 0) {
      this.claude.delete(id)
    } else {
      this.claude.set(id, remaining)
    }
    this.scheduleWrite()
  }

  /** The directory a shell was started in, standing in until the next refresh reads the live one. */
  seedDirectory(id: SessionId, cwd: string): void {
    if (this.frozen) {
      return
    }
    this.cwds.set(id, cwd)
  }

  /** A restored session, provisional until its SessionStart confirms it or PROVISIONAL_TTL_MS passes. */
  seedClaude(id: SessionId, session: ClaudeSession): void {
    if (this.frozen) {
      return
    }

    const entries = this.entriesFor(id)
    entries.push({ session, provisionalSince: this.now() })
  }

  /** The quitting flag; after it only flushAtQuit changes the file. */
  freeze(): void {
    this.frozen = true
  }

  /** The final write at quit, after a directory refresh bounded by QUIT_HOLD_MS; never rejects. */
  async flushAtQuit(): Promise<void> {
    let holdTimer: ReturnType<typeof setTimeout> | undefined
    try {
      if (this.latestWorkspace === null) {
        return
      }

      const hold = new Promise<void>((resolve) => {
        holdTimer = setTimeout(resolve, QUIT_HOLD_MS)
      })
      await Promise.race([this.refreshDirectories(), hold])
      this.writeNow()
    } catch {
      return
    } finally {
      clearTimeout(holdTimer)
    }
  }

  stop(): void {
    clearInterval(this.refreshTimer)
    if (this.debounceTimer !== null) {
      clearTimeout(this.debounceTimer)
      this.debounceTimer = null
    }
  }

  private scheduleWrite(): void {
    if (this.frozen) {
      return
    }
    if (this.debounceTimer !== null) {
      clearTimeout(this.debounceTimer)
    }

    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = null
      try {
        this.writeNow()
      } catch {
        return
      }
    }, SNAPSHOT_DEBOUNCE_MS)
  }

  private writeNow(): void {
    if (this.latestWorkspace === null) {
      return
    }

    this.dropExpiredClaude()
    const snapshot = snapshotFrom(
      this.latestWorkspace,
      (id) => this.cwdOf(id),
      (id) => this.claudeOf(id)
    )
    writeSnapshot(this.directory, snapshot)
  }

  private cwdOf(id: SessionId): string | null {
    const cwd = this.cwds.get(id)
    if (cwd === undefined) {
      return null
    }
    return cwd
  }

  private entriesFor(id: SessionId): ClaudeEntry[] {
    let entries = this.claude.get(id)
    if (entries === undefined) {
      entries = []
      this.claude.set(id, entries)
    }
    return entries
  }

  // A pane's newest session is the one it resumes: an older entry whose Claude
  // died without a SessionEnd would otherwise pin the pane to a dead id for good.
  private claudeOf(id: SessionId): ClaudeSession | null {
    const entries = this.claude.get(id)
    if (entries === undefined) {
      return null
    }
    if (entries.length === 0) {
      return null
    }
    const newest = entries[entries.length - 1]
    return newest.session
  }

  private dropExpiredClaude(): void {
    const now = this.now()
    for (const [id, entries] of this.claude) {
      const live = entries.filter((entry) => {
        if (entry.provisionalSince === null) {
          return true
        }
        return now - entry.provisionalSince <= PROVISIONAL_TTL_MS
      })
      if (live.length === 0) {
        this.claude.delete(id)
      } else {
        this.claude.set(id, live)
      }
    }
  }

  private paneIds(): SessionId[] {
    const ids: SessionId[] = []
    if (this.latestWorkspace === null) {
      return ids
    }
    for (const group of this.latestWorkspace.groups) {
      for (const pane of group.panes) {
        ids.push(pane.id)
      }
    }
    return ids
  }

  private async refreshDirectories(): Promise<void> {
    const ids = this.paneIds()
    const refreshes = ids.map((id) => this.refreshDirectory(id))
    await Promise.all(refreshes)
  }

  private async refreshDirectory(id: SessionId): Promise<void> {
    const pid = this.sessions.pidOf(id)
    if (pid === null) {
      return
    }
    const directory = await processCwd(pid)
    if (directory === null) {
      return
    }
    this.cwds.set(id, directory)
  }
}
