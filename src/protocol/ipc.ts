import type { AgentBanner } from './agentSetup'
import type { Config, ConfigFile } from './config'
import type { Workspace } from './arrangement'
import type { AgentSignal } from './rows'
import type { AgentSession, Snapshot } from './snapshot'

export type SessionId = string

export interface ForkRequest {
  from: SessionId
  name: string
}

export interface PtyCreateOptions {
  cwdFrom?: SessionId
  cwd?: string
  resume?: AgentSession
  fork?: ForkRequest
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

export interface AgentSignalEvent {
  id: SessionId
  harness: string
  sessionId: string
  signal: AgentSignal
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
  agentSignal: 'agent:signal',
  appActivated: 'app:activated',
  focusUnavailable: 'focus:unavailable',
  fullScreenGet: 'full-screen:get',
  fullScreenChanged: 'full-screen:changed',
  snapshotGet: 'snapshot:get',
  snapshotWorkspace: 'snapshot:workspace',
  updateGet: 'update:get',
  updateReady: 'update:ready',
  updateProblem: 'update:problem',
  updateApply: 'update:apply',
  agentsGet: 'agents:get',
  agentsChanged: 'agents:changed',
  agentsSetUp: 'agents:set-up',
  agentsDecline: 'agents:decline',
  agentsDismiss: 'agents:dismiss'
} as const

/** What the window asks of the Electron code itself: nothing here reaches a terminal or the workspace. */
export interface HostApi {
  clipboard: {
    read(): Promise<string>
    write(text: string): void
  }
  link: {
    open(url: string): void
  }
  fullScreen: {
    initial: boolean
    onChange(listener: (fullScreen: boolean) => void): () => void
  }
}

/** Everything the renderer may ask the main process to do, exposed on `window.tacoShells`. */
export interface TacoShellsApi extends HostApi {
  pty: {
    create(size: PtySize, options?: PtyCreateOptions): Promise<SessionId>
    write(id: SessionId, data: string): void
    resize(id: SessionId, size: PtySize): void
    kill(id: SessionId): void
    onData(listener: (event: PtyDataEvent) => void): () => void
    onExit(listener: (event: PtyExitEvent) => void): () => void
  }
  config: {
    initial: Config
    onChange(listener: (config: Config) => void): () => void
    pathOf(which: ConfigFile): string
  }
  focus: {
    onFocusSession(listener: (id: SessionId) => void): () => void
    onAttention(listener: (id: SessionId) => void): () => void
    onAgentSignal(listener: (event: AgentSignalEvent) => void): () => void
    onActivated(listener: () => void): () => void
    onUnavailable(listener: () => void): () => void
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
  agents: {
    initial: AgentBanner[]
    onChange(listener: (banners: AgentBanner[]) => void): () => void
    setUp(harness: string): void
    decline(harness: string): void
    dismiss(harness: string): void
  }
}
