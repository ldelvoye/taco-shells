import type { Workspace } from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'
import type { AgentSessions } from '../agentSessions'
import { readSnapshot, writeSnapshot } from './files'
import { snapshotFrom, type Snapshot } from './format'

export const SNAPSHOT_DEBOUNCE_MS = 250
export const CWD_REFRESH_MS = 30_000
export const QUIT_HOLD_MS = 1000

export class SnapshotStore {
  private readonly directory: string
  private readonly agentSessions: AgentSessions
  private readonly directoryOf: (id: SessionId) => Promise<string | null>
  private readonly initialSnapshot: Snapshot | null
  private readonly cwds = new Map<SessionId, string>()
  private readonly refreshTimer: ReturnType<typeof setInterval>
  private debounceTimer: ReturnType<typeof setTimeout> | null = null
  private latestWorkspace: Workspace | null = null
  private frozen = false

  constructor(
    directory: string,
    agentSessions: AgentSessions,
    directoryOf: (id: SessionId) => Promise<string | null>
  ) {
    this.directory = directory
    this.agentSessions = agentSessions
    this.directoryOf = directoryOf
    this.initialSnapshot = readSnapshot(directory)

    agentSessions.onChange(() => {
      this.scheduleWrite()
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

  /** The directory a shell was started in, standing in until the next refresh reads the live one. */
  seedDirectory(id: SessionId, cwd: string): void {
    if (this.frozen) {
      return
    }
    this.cwds.set(id, cwd)
  }

  forget(id: SessionId): void {
    if (this.frozen) {
      return
    }
    this.cwds.delete(id)
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

    this.agentSessions.dropExpired()
    const snapshot = snapshotFrom(
      this.latestWorkspace,
      (id) => this.cwdOf(id),
      (id) => this.agentSessions.sessionOf(id)
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
    const directory = await this.directoryOf(id)
    if (directory === null) {
      return
    }
    this.cwds.set(id, directory)
  }
}
