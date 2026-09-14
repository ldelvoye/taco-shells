import type { Config, ConfigFile } from './config'

export type SessionId = string

export interface PtySize {
  cols: number
  rows: number
}

export interface PtyDataEvent {
  id: SessionId
  data: string
}

export interface PtyExitEvent {
  id: SessionId
  exitCode: number
}

export const CHANNEL = {
  ptyCreate: 'pty:create',
  ptyWrite: 'pty:write',
  ptyResize: 'pty:resize',
  ptyKill: 'pty:kill',
  ptyData: 'pty:data',
  ptyExit: 'pty:exit',
  clipboardRead: 'clipboard:read',
  clipboardWrite: 'clipboard:write',
  configGet: 'config:get',
  configChanged: 'config:changed',
  configPath: 'config:path'
} as const

/** Everything the renderer may ask the main process to do, exposed on `window.tacoShells`. */
export interface TacoShellsApi {
  pty: {
    create(size: PtySize, cwdFrom?: SessionId): Promise<SessionId>
    write(id: SessionId, data: string): void
    resize(id: SessionId, size: PtySize): void
    kill(id: SessionId): void
    onData(listener: (event: PtyDataEvent) => void): () => void
    onExit(listener: (event: PtyExitEvent) => void): () => void
  }
  clipboard: {
    read(): Promise<string>
    write(text: string): void
  }
  config: {
    initial: Config
    onChange(listener: (config: Config) => void): () => void
    pathOf(which: ConfigFile): string
  }
}
