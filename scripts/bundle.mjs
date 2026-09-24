import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, rmSync } from 'node:fs'
import { basename, join } from 'node:path'

const OUTPUT_DIRECTORY = 'dist.noindex'
const APP_DIRECTORY = join(OUTPUT_DIRECTORY, 'mac-arm64')

// More than one match means something is stale; guessing would be worse than
// stopping. Finding the file rather than naming it also keeps this clear of
// electron-builder.js's identity and artifact naming.
function onlyFileEndingIn(directory, extension) {
  if (!existsSync(directory)) {
    return null
  }

  const entries = readdirSync(directory)
  const matches = entries.filter((entry) => entry.endsWith(extension))
  if (matches.length !== 1) {
    return null
  }

  return join(directory, matches[0])
}

/** The .app the build last wrote, whichever identity it was built under. */
export function builtBundle() {
  return onlyFileEndingIn(APP_DIRECTORY, '.app')
}

/** The dmg the build last wrote. */
export function builtDmg() {
  return onlyFileEndingIn(OUTPUT_DIRECTORY, '.dmg')
}

/** Removes the dmgs, and their blockmaps, that earlier builds left behind. */
export function clearBuiltDmgs() {
  if (!existsSync(OUTPUT_DIRECTORY)) {
    return
  }

  const entries = readdirSync(OUTPUT_DIRECTORY)
  const stale = entries.filter((entry) => entry.endsWith('.dmg') || entry.endsWith('.dmg.blockmap'))
  for (const entry of stale) {
    rmSync(join(OUTPUT_DIRECTORY, entry))
  }
}

/** Where a built bundle installs to, under the name it was built with. */
export function installedPathOf(bundle) {
  const name = basename(bundle)
  return join('/Applications', name)
}

// -f matches the whole command line, which is what finds an Electron app by its
// bundle path. Matching on the process name alone would silently never match.
/** Whether an installed app currently has a process running. */
export function appIsRunning(installedPath) {
  try {
    execFileSync('pgrep', ['-f', installedPath], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}
