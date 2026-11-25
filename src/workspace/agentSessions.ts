import type { SessionId } from '@protocol/ipc'
import type { AgentSession } from './snapshot/format'

export const PROVISIONAL_TTL_MS = 60_000

interface AgentEntry {
  session: AgentSession
  provisionalSince: number | null
}

export class AgentSessions {
  private readonly now: () => number
  private readonly agents = new Map<SessionId, AgentEntry[]>()
  private readonly changeListeners = new Set<() => void>()
  private frozen = false

  constructor(now: () => number = Date.now) {
    this.now = now
  }

  onChange(listener: () => void): void {
    this.changeListeners.add(listener)
  }

  /** An agent session that started in a pane; the newest one is what the pane resumes. */
  recordStart(id: SessionId, session: AgentSession): void {
    if (this.frozen) {
      return
    }

    const entries = this.entriesFor(id)
    const existing = entries.find((entry) => {
      return entry.session.harness === session.harness && entry.session.sessionId === session.sessionId
    })
    if (existing === undefined) {
      entries.push({ session, provisionalSince: null })
    } else {
      existing.provisionalSince = null
    }
    this.announceChange()
  }

  /** An agent session that ended in a pane, dropped from what the pane resumes. */
  recordEnd(id: SessionId, harness: string, sessionId: string): void {
    if (this.frozen) {
      return
    }

    const entries = this.agents.get(id)
    if (entries === undefined) {
      return
    }

    const remaining = entries.filter((entry) => {
      const isEnded = entry.session.harness === harness && entry.session.sessionId === sessionId
      return !isEnded
    })
    if (remaining.length === 0) {
      this.agents.delete(id)
    } else {
      this.agents.set(id, remaining)
    }
    this.announceChange()
  }

  /** A restored session, provisional until its SessionStart confirms it or PROVISIONAL_TTL_MS passes. */
  seed(id: SessionId, session: AgentSession): void {
    if (this.frozen) {
      return
    }

    const entries = this.entriesFor(id)
    entries.push({ session, provisionalSince: this.now() })
  }

  // A pane's newest session is the one it resumes or forks: an older entry whose
  // agent died without a SessionEnd would otherwise pin the pane to a dead id for good.
  sessionOf(id: SessionId): AgentSession | null {
    const entries = this.agents.get(id)
    if (entries === undefined) {
      return null
    }
    if (entries.length === 0) {
      return null
    }
    const newest = entries[entries.length - 1]
    return newest.session
  }

  forget(id: SessionId): void {
    if (this.frozen) {
      return
    }
    this.agents.delete(id)
  }

  dropExpired(): void {
    const now = this.now()
    for (const [id, entries] of this.agents) {
      const live = entries.filter((entry) => {
        if (entry.provisionalSince === null) {
          return true
        }
        return now - entry.provisionalSince <= PROVISIONAL_TTL_MS
      })
      if (live.length === 0) {
        this.agents.delete(id)
      } else {
        this.agents.set(id, live)
      }
    }
  }

  freeze(): void {
    this.frozen = true
  }

  private announceChange(): void {
    for (const listener of this.changeListeners) {
      listener()
    }
  }

  private entriesFor(id: SessionId): AgentEntry[] {
    let entries = this.agents.get(id)
    if (entries === undefined) {
      entries = []
      this.agents.set(id, entries)
    }
    return entries
  }
}
