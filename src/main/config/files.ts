import { mkdirSync, readFileSync, watch, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import type { ConfigFile } from '@shared/config'
import { configDirectoryName } from '../channel'

const SETTINGS_FILE = 'settings.json'
const KEYBINDINGS_FILE = 'keybindings.json'
const FOCUS_PORT_FILE = 'focus-port'
const WATCH_DEBOUNCE_MS = 100

export interface ConfigTexts {
  settings: string | null
  keybindings: string | null
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  } else {
    return String(error)
  }
}

function isFileExistsError(error: unknown): boolean {
  if (error instanceof Error && 'code' in error) {
    return error.code === 'EEXIST'
  } else {
    return false
  }
}

// TACO_SHELLS_CONFIG_DIR is what keeps the e2e suite out of the real config
// directory, which it would otherwise seed and read.
export function configDirectory(channel: string): string {
  const override = process.env.TACO_SHELLS_CONFIG_DIR
  let directory: string
  if (override) {
    directory = override
  } else {
    const home = homedir()
    directory = join(home, configDirectoryName(channel))
  }
  return directory
}

export function configFilePath(directory: string, which: ConfigFile): string {
  let fileName: string
  if (which === 'settings') {
    fileName = SETTINGS_FILE
  } else {
    fileName = KEYBINDINGS_FILE
  }
  return join(directory, fileName)
}

export function focusPortPath(directory: string): string {
  return join(directory, FOCUS_PORT_FILE)
}

function readConfigFile(path: string): string | null {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return null
  }
}

export function readConfigTexts(directory: string): ConfigTexts {
  const settingsPath = join(directory, SETTINGS_FILE)
  const settings = readConfigFile(settingsPath)
  const keybindingsPath = join(directory, KEYBINDINGS_FILE)
  const keybindings = readConfigFile(keybindingsPath)
  return { settings, keybindings }
}

function seedConfigFile(path: string, text: string, problems: string[]): void {
  try {
    writeFileSync(path, text, { flag: 'wx' })
  } catch (error) {
    if (isFileExistsError(error)) {
      reportIfUnreadable(path, problems)
      return
    }
    const message = errorMessage(error)
    problems.push(`${path} could not be created: ${message}`)
  }
}

// "Already there" covers a symlink pointing at nothing, which is left alone and
// then reads as no file at all. Saying so beats falling back to the defaults in
// silence.
function reportIfUnreadable(path: string, problems: string[]): void {
  const existing = readConfigFile(path)
  if (existing !== null) {
    return
  }
  problems.push(`${path} exists but could not be read`)
}

export function seedConfigFiles(directory: string, settingsText: string, keybindingsText: string): string[] {
  const problems: string[] = []

  try {
    mkdirSync(directory, { recursive: true })
  } catch (error) {
    const message = errorMessage(error)
    problems.push(`${directory} could not be created: ${message}`)
    return problems
  }

  const settingsPath = join(directory, SETTINGS_FILE)
  seedConfigFile(settingsPath, settingsText, problems)
  const keybindingsPath = join(directory, KEYBINDINGS_FILE)
  seedConfigFile(keybindingsPath, keybindingsText, problems)

  return problems
}

export function watchConfigDirectory(directory: string, onChange: () => void): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null

  try {
    const watcher = watch(directory, () => {
      if (timer) {
        clearTimeout(timer)
      }
      timer = setTimeout(onChange, WATCH_DEBOUNCE_MS)
    })

    return () => {
      if (timer) {
        clearTimeout(timer)
      }
      watcher.close()
    }
  } catch {
    return () => {}
  }
}
