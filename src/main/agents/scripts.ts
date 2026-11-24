import { chmodSync, copyFileSync, mkdirSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { errorMessage } from '../errorMessage'

const HOOKS_DIRECTORY_NAME = 'hooks'
const SCRIPT_MODE = 0o755

export function hooksDirectory(configDirectory: string): string {
  return join(configDirectory, HOOKS_DIRECTORY_NAME)
}

export function hookScriptsSource(isPackaged: boolean, resourcesPath: string, appPath: string): string {
  if (isPackaged) {
    return join(resourcesPath, HOOKS_DIRECTORY_NAME)
  } else {
    return join(appPath, HOOKS_DIRECTORY_NAME)
  }
}

export function copyHookScripts(source: string, destination: string): string[] {
  try {
    mkdirSync(destination, { recursive: true })
    const entries = readdirSync(source, { withFileTypes: true })
    for (const entry of entries) {
      if (!entry.isFile()) {
        continue
      }
      const from = join(source, entry.name)
      const to = join(destination, entry.name)
      copyFileSync(from, to)
      chmodSync(to, SCRIPT_MODE)
    }
    return []
  } catch (error) {
    const message = errorMessage(error)
    return [`The hook scripts could not be copied to ${destination}: ${message}`]
  }
}
