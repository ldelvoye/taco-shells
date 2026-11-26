import type { AgentBanner } from './agentSetup'
import type { Config, ConfigFile } from './config'
import type { AgentSignal } from './rows'
import type { TerminalEvent } from './runtimeMessages'
import type { DrawnState, RequestReply, WorkspaceRequest } from './workspace'

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

export interface AgentSignalEvent {
  id: SessionId
  harness: string
  sessionId: string
  signal: AgentSignal
}

export const CHANNEL = {
  runtimeConnect: 'runtime:connect',
  runtimePort: 'runtime:port',
  clipboardRead: 'clipboard:read',
  clipboardWrite: 'clipboard:write',
  linkOpen: 'link:open',
  configGet: 'config:get',
  configChanged: 'config:changed',
  configPath: 'config:path',
  focusUnavailable: 'focus:unavailable',
  fullScreenGet: 'full-screen:get',
  fullScreenChanged: 'full-screen:changed',
  workspaceGet: 'workspace:get',
  workspaceRequest: 'workspace:request',
  workspaceState: 'workspace:state',
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

/** Everything the window may ask of the app, exposed on `window.tacoShells`. */
export interface TacoShellsApi extends HostApi {
  pty: {
    write(id: SessionId, data: string): void
    resize(id: SessionId, size: PtySize): void
    attach(id: SessionId, size: PtySize | null): void
    detach(id: SessionId): void
    clear(id: SessionId): void
    onEvent(listener: (event: TerminalEvent) => void): () => void
  }
  config: {
    initial: Config
    onChange(listener: (config: Config) => void): () => void
    pathOf(which: ConfigFile): string
  }
  focus: {
    onUnavailable(listener: () => void): () => void
  }
  workspace: {
    initial: DrawnState
    request(request: WorkspaceRequest): Promise<RequestReply>
    onState(listener: (state: DrawnState) => void): () => void
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
