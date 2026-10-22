import { basename, dirname } from 'node:path'

const APPLICATIONS_PREFIX = '/Applications/'
const APP_SUFFIX = '.app'

/** The `.app` bundle in /Applications that an executable path belongs to, or null. */
export function installedBundlePath(executablePath: string): string | null {
  const macOsDirectory = dirname(executablePath)
  const contentsDirectory = dirname(macOsDirectory)
  const bundle = dirname(contentsDirectory)
  if (basename(macOsDirectory) !== 'MacOS' || basename(contentsDirectory) !== 'Contents') {
    return null
  }
  if (!bundle.endsWith(APP_SUFFIX) || !bundle.startsWith(APPLICATIONS_PREFIX)) {
    return null
  }
  return bundle
}

/** The arguments the update helper script takes: the pid to wait for, the installed app, the new app. */
export function helperArguments(pid: number, installed: string, extracted: string): string[] {
  return [String(pid), installed, extracted]
}
