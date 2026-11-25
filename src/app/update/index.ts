import { spawn } from 'node:child_process'
import { join } from 'node:path'
import { isStable } from '../channel'
import type { Service } from '../service'
import { helperArguments, installedBundlePath } from './apply'
import { UpdateChecker, type UpdateEvents } from './checker'
import { updatesDirectory } from './download'
import { isVersion } from './version'

const DEFAULT_FEED_URL = 'https://api.github.com/repos/ldelvoye/taco-shells/releases/latest'
const FEED_VARIABLE = 'TACO_SHELLS_UPDATE_FEED'
const HELPER_NAME = 'update-helper.sh'
const NOT_IN_APPLICATIONS = 'Taco Shells can only update itself from /Applications.'
const UNVERSIONED = '0.0.0'

export interface UpdateHost {
  appVersion: string
  executablePath: string
  resourcesPath: string
  fetch(input: string | Request, init?: RequestInit): Promise<Response>
  quit(): void
}

export interface UpdateService extends Service {
  checkNow(): void
  readyVersion(): string | null
  applyReady(): void
}

// The stable build always follows GitHub. The feed override is a test seam for
// the dev channel, which otherwise never checks, and it treats the dev build's
// unversioned number as older than any release.
function resolveFeedUrl(): string | null {
  if (isStable) {
    return DEFAULT_FEED_URL
  }
  const override = process.env[FEED_VARIABLE]
  if (override === undefined) {
    return null
  }
  return override
}

function resolveCurrentVersion(appVersion: string): string | null {
  if (isVersion(appVersion)) {
    return appVersion
  }
  if (isStable) {
    return null
  }
  return UNVERSIONED
}

/** The update checks and applying a ready update. In a build that does not update, both do nothing. */
export function startUpdates(configDirectory: string, host: UpdateHost, events: UpdateEvents): UpdateService {
  const feedUrl = resolveFeedUrl()
  const currentVersion = resolveCurrentVersion(host.appVersion)
  if (feedUrl === null || currentVersion === null) {
    return {
      stop: () => undefined,
      checkNow: () => undefined,
      readyVersion: () => null,
      applyReady: () => undefined
    }
  }

  const started = new UpdateChecker({
    feedUrl,
    currentVersion,
    updatesDirectory: updatesDirectory(configDirectory),
    fetchImpl: (input, init) => {
      if (input instanceof URL) {
        return host.fetch(input.href, init)
      }
      return host.fetch(input, init)
    },
    events
  })

  started.start()

  return {
    stop: () => {
      started.stop()
    },
    checkNow: () => {
      started.checkNow()
    },
    readyVersion: () => {
      const ready = started.readyApp()
      if (ready === null) {
        return null
      }
      return ready.version
    },
    applyReady: () => {
      const ready = started.readyApp()
      if (ready === null) {
        return
      }

      const installed = installedBundlePath(host.executablePath)
      if (installed === null) {
        events.onProblem(NOT_IN_APPLICATIONS)
        return
      }

      const helperPath = join(host.resourcesPath, HELPER_NAME)
      const helperArgs = helperArguments(process.pid, installed, ready.path)
      const child = spawn('/bin/sh', [helperPath, ...helperArgs], { detached: true, stdio: 'ignore' })
      child.unref()
      host.quit()
    }
  }
}
