import type { Config, ConfigFile } from './config'
import type { Workspace } from './model'
import type { ClaudeSession, Snapshot } from './snapshot'

export type SessionId = string

export interface PtyCreateOptions {
  cwdFrom?: SessionId
  cwd?: string
  resume?: ClaudeSession
}

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
  linkOpen: 'link:open',
  configGet: 'config:get',
  configChanged: 'config:changed',
  configPath: 'config:path',
  focusSession: 'focus:session',
  attentionSession: 'attention:session',
  appActivated: 'app:activated',
  focusUnavailable: 'focus:unavailable',
  fullScreenGet: 'full-screen:get',
  fullScreenChanged: 'full-screen:changed',
  snapshotGet: 'snapshot:get',
  snapshotWorkspace: 'snapshot:workspace',
  updateGet: 'update:get',
  updateReady: 'update:ready',
  updateProblem: 'update:problem',
  updateApply: 'update:apply'
} as const

/** Everything the renderer may ask the main process to do, exposed on `window.tacoShells`. */
export interface TacoShellsApi {
  pty: {
    create(size: PtySize, options?: PtyCreateOptions): Promise<SessionId>
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
  link: {
    open(url: string): void
  }
  config: {
    initial: Config
    onChange(listener: (config: Config) => void): () => void
    pathOf(which: ConfigFile): string
  }
  focus: {
    onFocusSession(listener: (id: SessionId) => void): () => void
    onAttention(listener: (id: SessionId) => void): () => void
    onActivated(listener: () => void): () => void
    onUnavailable(listener: () => void): () => void
  }
  fullScreen: {
    initial: boolean
    onChange(listener: (fullScreen: boolean) => void): () => void
  }
  snapshot: {
    initial: Snapshot | null
    workspaceChanged(workspace: Workspace): void
  }
  update: {
    initial: string | null
    onReady(listener: (version: string) => void): () => void
    onProblem(listener: (message: string) => void): () => void
    apply(): void
  }
}
