import { nativeTheme } from 'electron'
import type { Config } from '@shared/config'
import { DEFAULT_KEYBINDINGS_TEXT, DEFAULT_SETTINGS_TEXT } from './defaults'
import { type ConfigTexts, readConfigTexts, seedConfigFiles, watchConfigDirectory } from './files'
import { resolveConfig } from './resolve'

type ConfigListener = (config: Config) => void

export class ConfigStore {
  private readonly directory: string
  private readonly seedingProblems: string[]
  private readonly listeners = new Set<ConfigListener>()
  private readonly stopWatching: () => void
  private readonly onNativeThemeUpdate: () => void
  private config: Config
  private lastSerialized: string

  constructor(directory: string) {
    this.directory = directory
    this.seedingProblems = seedConfigFiles(directory, DEFAULT_SETTINGS_TEXT, DEFAULT_KEYBINDINGS_TEXT)
    this.config = this.resolve()
    this.lastSerialized = JSON.stringify(this.config)

    this.stopWatching = watchConfigDirectory(directory, () => {
      this.refresh()
    })
    this.onNativeThemeUpdate = () => {
      this.refresh()
    }
    nativeTheme.on('updated', this.onNativeThemeUpdate)
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
    nativeTheme.off('updated', this.onNativeThemeUpdate)
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
      systemPrefersDark: nativeTheme.shouldUseDarkColors
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
