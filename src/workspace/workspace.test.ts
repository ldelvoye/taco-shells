import { describe, expect, it } from 'vitest'

import { claudeForkCommand, claudeResumeCommand } from '@agents/claude/commands'
import { DEFAULT_HARNESS, harnessOf } from '@agents/registry'
import type { Workspace as Arrangement } from '@protocol/arrangement'
import type { PtyExitEvent, PtySize, SessionId } from '@protocol/ipc'
import type { Runtime, TerminalStart, TerminalStarted } from '@protocol/runtime'
import type { AgentSession, Snapshot } from './snapshot/format'
import type { DrawnState } from '@protocol/workspace'
import { AgentSessions } from './agentSessions'
import { Workspace } from './workspace'

const SESSION = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'
const OTHER_SESSION = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'
const INITIAL_SIZE: PtySize = { cols: 80, rows: 24 }

class FakeRuntime implements Runtime {
  readonly opened: TerminalStart[] = []
  readonly closed: SessionId[] = []
  onOpen: (start: TerminalStart) => void = () => undefined
  private readonly exitListeners: ((event: PtyExitEvent) => void)[] = []
  private readonly startedListeners: ((event: TerminalStarted) => void)[] = []
  private readonly titleListeners: ((id: SessionId, title: string) => void)[] = []

  open(start: TerminalStart): void {
    this.onOpen(start)
    this.opened.push(start)
  }

  close(id: SessionId): void {
    this.closed.push(id)
  }

  write(): void {
    return
  }

  resize(): void {
    return
  }

  setScrollback(): void {
    return
  }

  setColors(): void {
    return
  }

  onStarted(listener: (event: TerminalStarted) => void): void {
    this.startedListeners.push(listener)
  }

  onExit(listener: (event: PtyExitEvent) => void): void {
    this.exitListeners.push(listener)
  }

  onTitle(listener: (id: SessionId, title: string) => void): void {
    this.titleListeners.push(listener)
  }

  onSubmit(): void {
    return
  }

  directoryOf(): Promise<string | null> {
    return Promise.resolve(null)
  }

  foregroundCommands(): Promise<string[]> {
    return Promise.resolve([])
  }

  terminalForPids(): Promise<SessionId | null> {
    return Promise.resolve(null)
  }

  fireExit(id: SessionId): void {
    for (const listener of this.exitListeners) {
      listener({ id, exitCode: 0 })
    }
  }

  fireTitle(id: SessionId, title: string): void {
    for (const listener of this.titleListeners) {
      listener(id, title)
    }
  }

  fireStarted(id: SessionId, directory: string): void {
    for (const listener of this.startedListeners) {
      listener({ id, directory })
    }
  }
}

interface Recording {
  workspace: Workspace
  runtime: FakeRuntime
  agentSessions: AgentSessions
  states: DrawnState[]
  arrangements: Arrangement[]
  emptied: { count: number }
  startedIn: TerminalStarted[]
  exited: SessionId[]
}

function recordingWorkspace(snapshot: Snapshot | null = null): Recording {
  const states: DrawnState[] = []
  const arrangements: Arrangement[] = []
  const emptied = { count: 0 }
  const startedIn: TerminalStarted[] = []
  const exited: SessionId[] = []
  const runtime = new FakeRuntime()
  const agentSessions = new AgentSessions()
  const options = {
    snapshot,
    runtime,
    configPathOf: (file: string) => `/config/${file}.json`,
    agentSessions
  }
  const workspace = new Workspace(options, {
    onState: (state) => {
      states.push(state)
    },
    onArrangement: (arrangement) => {
      arrangements.push(arrangement)
    },
    onEmptied: () => {
      emptied.count += 1
    },
    onStartedIn: (id, directory) => {
      startedIn.push({ id, directory })
    },
    onExited: (id) => {
      exited.push(id)
    }
  })
  return { workspace, runtime, agentSessions, states, arrangements, emptied, startedIn, exited }
}

function opened(count: number): Recording {
  const recording = recordingWorkspace()
  recording.workspace.start()
  for (let opened = 1; opened < count; opened += 1) {
    recording.workspace.request({ type: 'openTerminal', file: null })
  }
  return recording
}

function groupIdsOf(workspace: Workspace): string[] {
  const arrangement = workspace.drawn().workspace
  if (arrangement === null) {
    return []
  }
  return arrangement.groups.map((group) => group.id)
}

function paneIdsOf(workspace: Workspace): string[][] {
  const arrangement = workspace.drawn().workspace
  if (arrangement === null) {
    return []
  }
  return arrangement.groups.map((group) => group.panes.map((pane) => pane.id))
}

function activePaneIdOf(workspace: Workspace): string | undefined {
  const arrangement = workspace.drawn().workspace
  if (arrangement === null) {
    return undefined
  }
  const group = arrangement.groups[arrangement.activeGroup]
  return group.panes[group.activePane].id
}

describe('Workspace', () => {
  it('opens a terminal under an id it mints, placed before the runtime is asked', () => {
    const { workspace, runtime, states } = recordingWorkspace()
    const heldWhenOpened: string[][][] = []
    runtime.onOpen = () => {
      heldWhenOpened.push(paneIdsOf(workspace))
    }

    const first = workspace.request({ type: 'openTerminal', file: null })
    const second = workspace.request({ type: 'openTerminal', file: null })

    expect(first).toEqual({ ok: true, tabGroup: null, revision: 1 })
    expect(second).toEqual({ ok: true, tabGroup: null, revision: 2 })
    expect(groupIdsOf(workspace)).toEqual(['group-1', 'group-2'])
    expect(states.map((state) => state.revision)).toEqual([1, 2])
    expect(runtime.opened.map((start) => [start.id, start.size])).toEqual([
      ['pty-1', INITIAL_SIZE],
      ['pty-2', INITIAL_SIZE]
    ])
    expect(heldWhenOpened).toEqual([[['pty-1']], [['pty-1'], ['pty-2']]])
  })

  it("opens a config file in the shell's editor", () => {
    const { workspace, runtime } = recordingWorkspace()

    workspace.request({ type: 'openTerminal', file: 'settings' })

    expect(runtime.opened).toHaveLength(1)
    expect(runtime.opened[0].command).toContain('/config/settings.json')
  })

  it("splits into the donor's directory, and refuses when there is no room", () => {
    const { workspace, runtime } = opened(1)

    const split = workspace.request({ type: 'splitTerminal', pane: 'pty-1' })
    expect(split.ok).toBe(true)
    expect(paneIdsOf(workspace)).toEqual([['pty-1', 'pty-2']])
    expect(runtime.opened[1]).toMatchObject({ id: 'pty-2', directory: null, directoryOf: 'pty-1' })

    for (let splits = 0; splits < 8; splits += 1) {
      workspace.request({ type: 'splitTerminal', pane: 'pty-1' })
    }
    const openedBefore = runtime.opened.length
    const refused = workspace.request({ type: 'splitTerminal', pane: 'pty-1' })
    expect(refused.ok).toBe(false)
    const refusedFork = workspace.request({ type: 'forkTerminal', pane: 'pty-1' })
    expect(refusedFork.ok).toBe(false)
    expect(runtime.opened.length).toBe(openedBefore)

    const unknown = workspace.request({ type: 'splitTerminal', pane: 'gone' })
    expect(unknown.ok).toBe(false)
    expect(runtime.opened.length).toBe(openedBefore)
  })

  it("forks under the next free fork name, with the agent's fork command", () => {
    const { workspace, runtime, agentSessions } = opened(1)
    const session: AgentSession = { harness: 'claude', sessionId: SESSION, cwd: '/work' }
    agentSessions.recordStart('pty-1', session)
    runtime.fireTitle('pty-1', 'Tacos')

    workspace.request({ type: 'forkTerminal', pane: 'pty-1' })
    runtime.fireTitle('pty-2', '⑂ Tacos')
    workspace.request({ type: 'forkTerminal', pane: 'pty-1' })

    expect(runtime.opened[1]).toMatchObject({
      id: 'pty-2',
      directory: '/work',
      directoryOf: null,
      command: claudeForkCommand(SESSION, '⑂ Tacos')
    })
    expect(runtime.opened[2]).toMatchObject({
      id: 'pty-3',
      command: claudeForkCommand(SESSION, '⑂2 Tacos')
    })

    const plain = opened(1)
    const forked = plain.workspace.request({ type: 'forkTerminal', pane: 'pty-1' })
    expect(forked.ok).toBe(true)
    expect(plain.runtime.opened[1]).toMatchObject({
      id: 'pty-2',
      directory: null,
      directoryOf: 'pty-1',
      command: null
    })
  })

  it('closes through the runtime and drops the terminal when its exit arrives', () => {
    const { workspace, runtime, agentSessions, emptied, exited } = opened(1)
    const session: AgentSession = { harness: 'claude', sessionId: SESSION, cwd: '/work' }
    workspace.agentStarted('pty-1', session, false)

    workspace.request({ type: 'closeTerminal', pane: 'pty-1' })
    expect(runtime.closed).toEqual(['pty-1'])
    expect(paneIdsOf(workspace)).toEqual([['pty-1']])
    expect(agentSessions.sessionOf('pty-1')).toEqual(session)

    runtime.fireExit('pty-1')
    expect(paneIdsOf(workspace)).toEqual([])
    expect(exited).toEqual(['pty-1'])
    expect(agentSessions.sessionOf('pty-1')).toBeNull()
    expect(emptied.count).toBe(1)
  })

  it('tells the save file where a shell started', () => {
    const { runtime, startedIn } = opened(1)

    runtime.fireStarted('pty-1', '/somewhere')

    expect(startedIn).toEqual([{ id: 'pty-1', directory: '/somewhere' }])
  })

  it('restores the saved arrangement and opens every pane where it was saved', () => {
    const agent: AgentSession = { harness: 'claude', sessionId: SESSION, cwd: '/work/b' }
    const snapshot: Snapshot = {
      version: 1,
      groups: [
        { panes: [{ width: 1, cwd: '/work/a', agent: null, color: null }], activePane: 0, tabGroup: null },
        { panes: [{ width: 1, cwd: '/work/b', agent, color: null }], activePane: 0, tabGroup: null }
      ],
      activeGroup: 0,
      tabGroups: []
    }
    const { workspace, runtime, agentSessions, startedIn } = recordingWorkspace(snapshot)

    workspace.start()

    expect(startedIn).toEqual([
      { id: 'pty-1', directory: '/work/a' },
      { id: 'pty-2', directory: '/work/b' }
    ])
    expect(paneIdsOf(workspace)).toEqual([['pty-1'], ['pty-2']])
    expect(groupIdsOf(workspace)).toEqual(['group-1', 'group-2'])
    expect(runtime.opened).toEqual([
      { id: 'pty-1', directory: '/work/a', directoryOf: null, command: null, size: INITIAL_SIZE },
      {
        id: 'pty-2',
        directory: '/work/b',
        directoryOf: null,
        command: claudeResumeCommand(SESSION),
        size: INITIAL_SIZE
      }
    ])
    expect(agentSessions.sessionOf('pty-2')).toEqual(agent)
    expect(agentSessions.sessionOf('pty-1')).toBeNull()
  })

  it('opens one fresh terminal when the saved arrangement cannot be rebuilt, and starts nothing else', () => {
    const unusable: Snapshot = { version: 1, groups: [], activeGroup: 0, tabGroups: [] }
    const { workspace, runtime, agentSessions } = recordingWorkspace(unusable)

    workspace.start()

    expect(runtime.opened).toHaveLength(1)
    expect(paneIdsOf(workspace)).toEqual([[runtime.opened[0].id]])
    expect(agentSessions.sessionOf(runtime.opened[0].id)).toBeNull()
  })

  it('opens one fresh terminal when there is nothing to restore', () => {
    const { workspace, runtime } = recordingWorkspace()

    workspace.start()

    expect(paneIdsOf(workspace)).toEqual([['pty-1']])
    expect(runtime.opened).toEqual([
      { id: 'pty-1', directory: null, directoryOf: null, command: null, size: INITIAL_SIZE }
    ])
  })

  it("records an agent's start and end, and reports each as a signal", () => {
    const { workspace, agentSessions } = opened(1)
    const first: AgentSession = { harness: 'claude', sessionId: SESSION, cwd: '/work' }
    const second: AgentSession = { harness: 'claude', sessionId: OTHER_SESSION, cwd: '/work' }

    workspace.agentStarted('pty-1', first, false)
    expect(agentSessions.sessionOf('pty-1')).toEqual(first)
    expect(workspace.drawn().agentRows['pty-1']).toEqual({
      harness: 'claude',
      sessionId: SESSION,
      state: 'idle',
      hasConversation: false
    })

    workspace.agentStarted('pty-1', second, true)
    expect(agentSessions.sessionOf('pty-1')).toEqual(second)
    expect(workspace.drawn().agentRows['pty-1']).toMatchObject({
      sessionId: OTHER_SESSION,
      hasConversation: true
    })

    workspace.agentEnded('pty-1', 'claude', OTHER_SESSION)
    expect(agentSessions.sessionOf('pty-1')).toEqual(first)
    expect(workspace.drawn().agentRows['pty-1']).toBeUndefined()
  })

  it("applies the window's focus move and says which one it has applied", () => {
    const { workspace, states } = opened(2)
    expect(activePaneIdOf(workspace)).toBe('pty-2')

    workspace.request({ type: 'focusReported', pane: 'pty-1', count: 3 })
    expect(activePaneIdOf(workspace)).toBe('pty-1')
    expect(states[states.length - 1].focusCount).toBe(3)

    const statesBefore = states.length
    const arrangementBefore = workspace.drawn().workspace
    workspace.request({ type: 'focusReported', pane: 'pty-1', count: 4 })
    expect(states.length).toBe(statesBefore + 1)
    expect(states[states.length - 1].focusCount).toBe(4)
    expect(workspace.drawn().workspace).toBe(arrangementBefore)

    workspace.request({ type: 'focusReported', pane: 'pty-1', count: 2 })
    expect(workspace.drawn().focusCount).toBe(4)
    expect(states.length).toBe(statesBefore + 1)
  })

  it('creates a tab group and answers with its id', () => {
    const { workspace } = opened(1)

    const created = workspace.request({ type: 'tabGroupCreated', pane: 'pty-1' })
    expect(created.tabGroup).toBe('tab-group-1')

    const unknown = workspace.request({ type: 'tabGroupCreated', pane: 'gone' })
    expect(unknown.tabGroup).toBeNull()
  })

  it("mints the new entry's id for a drop", () => {
    const { workspace } = opened(1)
    workspace.request({ type: 'splitTerminal', pane: 'pty-1' })

    workspace.request({
      type: 'dropped',
      dragged: { kind: 'pane', pane: 'pty-2' },
      drop: { kind: 'between', index: 1, tabGroup: null }
    })

    expect(groupIdsOf(workspace)).toEqual(['group-1', 'group-2'])
    expect(paneIdsOf(workspace)).toEqual([['pty-1'], ['pty-2']])
  })

  it('publishes rows as a plain object', () => {
    const { workspace } = opened(1)

    workspace.agentSignalled({ id: 'pty-1', harness: 'claude', sessionId: SESSION, signal: 'start' })

    const drawn = workspace.drawn()
    expect(Object.keys(drawn.agentRows)).toEqual(['pty-1'])
    expect(JSON.parse(JSON.stringify(drawn))).toEqual(drawn)
  })

  it('says when the last terminal has gone, once', () => {
    const { runtime, emptied } = opened(1)

    runtime.fireExit('pty-1')
    expect(emptied.count).toBe(1)

    runtime.fireExit('pty-1')
    expect(emptied.count).toBe(1)
  })

  it('when the runtime dies every terminal goes, the window is told why, and nothing closes or reopens', () => {
    const { workspace, runtime, states, emptied } = opened(2)
    const openedBefore = runtime.opened.length
    const closedBefore = runtime.closed.length
    const revisionBefore = workspace.drawn().revision

    workspace.runtimeDied('code 9')

    const told = states[states.length - 1]
    expect(told.workspace?.groups.length).toBe(0)
    expect(told.problem).toContain('code 9')
    expect(told.revision).toBeGreaterThan(revisionBefore)
    expect(emptied.count).toBe(0)
    expect(runtime.closed.length).toBe(closedBefore)

    const reply = workspace.request({ type: 'openTerminal', file: null })
    const replyInGroup = workspace.request({ type: 'openTerminalIn', tabGroup: null })
    expect(reply.ok).toBe(false)
    expect(replyInGroup.ok).toBe(false)
    expect(runtime.opened.length).toBe(openedBefore)
  })

  it('hands the keyboard back when a hook asks for a terminal that is already active', () => {
    const { workspace, states } = opened(2)
    const before = workspace.drawn()

    workspace.focusRequested('pty-2')
    const after = workspace.drawn()
    expect(after.refocus).toBe(before.refocus + 1)
    expect(after.workspace).toBe(before.workspace)

    const statesBefore = states.length
    workspace.appActivated()
    expect(states.length).toBe(statesBefore)

    workspace.attentionRequested('pty-1')
    workspace.appActivated()
    expect(activePaneIdOf(workspace)).toBe('pty-1')
    expect(workspace.drawn().refocus).toBe(after.refocus + 1)
  })

  it('reports the arrangement to save only when it changed', () => {
    const { workspace, runtime, states, arrangements } = opened(1)
    const savesBefore = arrangements.length

    runtime.fireTitle('pty-1', 'renamed')
    expect(arrangements.length).toBe(savesBefore + 1)

    const statesBefore = states.length
    runtime.fireTitle('gone', 'renamed')
    expect(arrangements.length).toBe(savesBefore + 1)
    expect(states.length).toBe(statesBefore)

    workspace.agentSignalled({ id: 'pty-1', harness: 'claude', sessionId: SESSION, signal: 'start' })
    expect(arrangements.length).toBe(savesBefore + 1)
  })

  it("tells the window each agent's menu name and whether it forks", () => {
    const { workspace } = opened(1)
    const claude = harnessOf('claude')

    const drawn = workspace.drawn()
    expect(claude).not.toBeNull()
    expect(drawn.agents.claude).toEqual({ name: claude?.name, forks: true })
    expect(drawn.defaultAgentName).toBe(DEFAULT_HARNESS.name)
  })
})
