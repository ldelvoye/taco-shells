import type { Config } from '@protocol/config'
import { DEFAULT_KEYBINDINGS_TEXT, DEFAULT_SETTINGS_TEXT } from './defaults'
import { type ConfigTexts, readConfigTexts, seedConfigFiles, watchConfigDirectory } from './files'
import { resolveConfig } from './resolve'

type ConfigListener = (config: Config) => void

export interface SystemTheme {
  prefersDark(): boolean
  onChange(listener: () => void): () => void
}

export class ConfigStore {
  private readonly directory: string
  private readonly systemTheme: SystemTheme
  private readonly seedingProblems: string[]
  private readonly listeners = new Set<ConfigListener>()
  private readonly stopWatching: () => void
  private readonly stopWatchingTheme: () => void
  private config: Config
  private lastSerialized: string

  constructor(directory: string, systemTheme: SystemTheme) {
    this.directory = directory
    this.systemTheme = systemTheme
    this.seedingProblems = seedConfigFiles(directory, DEFAULT_SETTINGS_TEXT, DEFAULT_KEYBINDINGS_TEXT)
    this.config = this.resolve()
    this.lastSerialized = JSON.stringify(this.config)

    this.stopWatching = watchConfigDirectory(directory, () => {
      this.refresh()
    })
    this.stopWatchingTheme = systemTheme.onChange(() => {
      this.refresh()
    })
  }

  current(): Config {
    return this.config
  }

  /** The directory this store reads its files from. */
  configDirectory(): string {
    return this.directory
  }

  onChange(listener: ConfigListener): void {
    this.listeners.add(listener)
  }

  stop(): void {
    this.stopWatching()
    this.stopWatchingTheme()
  }

  // Resolution runs inside the directory watcher, where anything thrown would
  // take the whole process down and every shell with it. The user's files are
  // the only untrusted input, so failing to read them costs them and nothing
  // else.
  private resolve(): Config {
    try {
      return this.resolveWith(readConfigTexts(this.directory))
    } catch {
      const withoutUserFiles = this.resolveWith({ settings: null, keybindings: null })
      const problems = [...withoutUserFiles.problems, 'the config files could not be read']
      return { ...withoutUserFiles, problems }
    }
  }

  private resolveWith(texts: ConfigTexts): Config {
    const resolved = resolveConfig({
      defaultKeybindingsText: DEFAULT_KEYBINDINGS_TEXT,
      userSettingsText: texts.settings,
      userKeybindingsText: texts.keybindings,
      systemPrefersDark: this.systemTheme.prefersDark()
    })
    const problems = [...this.seedingProblems, ...resolved.problems]
    return { ...resolved, problems }
  }

  private refresh(): void {
    const config = this.resolve()
    const serialized = JSON.stringify(config)

    // The seeding write trips our own watcher and editors often save twice, so
    // the comparison is what keeps those from reaching the renderer.
    if (serialized === this.lastSerialized) {
      return
    }

    this.config = config
    this.lastSerialized = serialized
    for (const listener of this.listeners) {
      listener(config)
    }
  }
}
