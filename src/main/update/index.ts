import { spawn } from 'node:child_process'
import { join } from 'node:path'
import { app, type BrowserWindow, ipcMain, net } from 'electron'
import { CHANNEL } from '@shared/ipc'
import { isVersion } from '@shared/version'
import { isStable } from '../channel'
import { helperArguments, installedBundlePath } from './apply'
import { UpdateChecker, type UpdateEvents } from './checker'
import { updatesDirectory } from './download'

const DEFAULT_FEED_URL = 'https://api.github.com/repos/ldelvoye/taco-shells/releases/latest'
const FEED_VARIABLE = 'TACO_SHELLS_UPDATE_FEED'
const HELPER_NAME = 'update-helper.sh'
const NOT_IN_APPLICATIONS = 'Taco Shells can only update itself from /Applications.'
const UNVERSIONED = '0.0.0'

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

function resolveCurrentVersion(): string | null {
  const version = app.getVersion()
  if (isVersion(version)) {
    return version
  }
  if (isStable) {
    return null
  }
  return UNVERSIONED
}

function sendToWindow(window: BrowserWindow, channel: string, payload: string): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel, payload)
}

/** The update checks and the apply handler, or null when this build does not update. */
export function registerUpdates(
  window: BrowserWindow,
  configDirectory: string
): (() => void) | null {
  let checker: UpdateChecker | null = null

  // A sendSync left unanswered hangs the renderer at startup, so this answers
  // whether or not a checker exists.
  ipcMain.on(CHANNEL.updateGet, (event) => {
    if (checker === null) {
      event.returnValue = null
      return
    }
    const ready = checker.readyApp()
    if (ready === null) {
      event.returnValue = null
    } else {
      event.returnValue = ready.version
    }
  })

  const feedUrl = resolveFeedUrl()
  const currentVersion = resolveCurrentVersion()
  if (feedUrl === null || currentVersion === null) {
    return null
  }

  const events: UpdateEvents = {
    onReady: (version: string) => {
      sendToWindow(window, CHANNEL.updateReady, version)
    },
    onProblem: (message: string) => {
      sendToWindow(window, CHANNEL.updateProblem, message)
    }
  }

  const started = new UpdateChecker({
    feedUrl,
    currentVersion,
    updatesDirectory: updatesDirectory(configDirectory),
    fetchImpl: (input, init) => {
      if (input instanceof URL) {
        return net.fetch(input.href, init)
      }
      return net.fetch(input, init)
    },
    events
  })
  checker = started

  ipcMain.on(CHANNEL.updateApply, () => {
    const ready = started.readyApp()
    if (ready === null) {
      return
    }

    const installed = installedBundlePath(app.getPath('exe'))
    if (installed === null) {
      events.onProblem(NOT_IN_APPLICATIONS)
      return
    }

    const helperPath = join(process.resourcesPath, HELPER_NAME)
    const helperArgs = helperArguments(process.pid, installed, ready.path)
    const child = spawn('/bin/sh', [helperPath, ...helperArgs], { detached: true, stdio: 'ignore' })
    child.unref()
    app.quit()
  })

  started.start()

  return () => {
    started.stop()
    ipcMain.removeAllListeners(CHANNEL.updateApply)
    ipcMain.removeAllListeners(CHANNEL.updateGet)
  }
}
