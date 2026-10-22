import { join } from 'node:path'
import { isNewerVersion } from '@shared/version'
import {
  downloadAndExtract,
  extractedAppIn,
  listChildren,
  removeOtherVersions,
  removeVersion
} from './download'
import { parseRelease } from './feed'

export const FIRST_CHECK_DELAY_MS = 10_000
export const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000

export interface UpdateEvents {
  onReady(version: string): void
  onProblem(message: string): void
}

interface UpdateCheckerOptions {
  feedUrl: string
  currentVersion: string
  updatesDirectory: string
  fetchImpl: typeof fetch
  events: UpdateEvents
}

interface ReadyApp {
  version: string
  path: string
}

/** The timer-driven lookout for newer releases, holding the newest extracted app ready to install. */
export class UpdateChecker {
  private readonly options: UpdateCheckerOptions
  private ready: ReadyApp | null = null
  private checking = false
  private firstTimer: ReturnType<typeof setTimeout> | null = null
  private intervalTimer: ReturnType<typeof setInterval> | null = null

  constructor(options: UpdateCheckerOptions) {
    this.options = options
  }

  /** The first prune and adoption of what is already on disk, then the scheduled checks. */
  start(): void {
    this.pruneAndAdopt()
    this.firstTimer = setTimeout(() => this.runScheduledCheck(), FIRST_CHECK_DELAY_MS)
    this.intervalTimer = setInterval(() => this.runScheduledCheck(), CHECK_INTERVAL_MS)
  }

  stop(): void {
    if (this.firstTimer !== null) {
      clearTimeout(this.firstTimer)
      this.firstTimer = null
    }
    if (this.intervalTimer !== null) {
      clearInterval(this.intervalTimer)
      this.intervalTimer = null
    }
  }

  /** The downloaded app waiting to be installed, or null. */
  readyApp(): ReadyApp | null {
    return this.ready
  }

  private pruneAndAdopt(): void {
    try {
      const { updatesDirectory, currentVersion } = this.options
      const names = listChildren(updatesDirectory)
      for (const name of names) {
        const isNewer = isNewerVersion(name, currentVersion)
        if (!isNewer) {
          removeVersion(updatesDirectory, name)
          continue
        }
        this.adoptExtracted(name)
      }
    } catch {
      return
    }
  }

  private adoptExtracted(version: string): void {
    const directory = join(this.options.updatesDirectory, version)
    const path = extractedAppIn(directory)
    if (path === null) {
      removeVersion(this.options.updatesDirectory, version)
      return
    }
    if (this.ready !== null && !isNewerVersion(version, this.ready.version)) {
      return
    }
    this.ready = { version, path }
    this.options.events.onReady(version)
  }

  private runScheduledCheck(): void {
    this.check().catch(() => undefined)
  }

  private async check(): Promise<void> {
    if (this.checking) {
      return
    }
    this.checking = true
    try {
      await this.fetchAndDownload()
    } catch {
      return
    } finally {
      this.checking = false
    }
  }

  private async fetchAndDownload(): Promise<void> {
    const { feedUrl, currentVersion, updatesDirectory, fetchImpl, events } = this.options
    const response = await fetchImpl(feedUrl, { cache: 'no-store' })
    const payload: unknown = await response.json()
    const release = parseRelease(payload)
    if (release === null) {
      return
    }
    if (!isNewerVersion(release.version, currentVersion)) {
      return
    }
    if (this.ready !== null && !isNewerVersion(release.version, this.ready.version)) {
      return
    }
    const path = await downloadAndExtract(release, updatesDirectory, fetchImpl)
    if (path === null) {
      return
    }
    this.ready = { version: release.version, path }
    removeOtherVersions(updatesDirectory, release.version)
    events.onReady(release.version)
  }
}
