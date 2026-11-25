import { describe, expect, it } from 'vitest'

import type { Group, Workspace } from '@protocol/arrangement'
import type { TacoShellsApi } from '@protocol/ipc'
import type { DrawnState, RequestReply, WorkspaceRequest } from '@protocol/workspace'
import { activePane } from '@rules/arrangement'
import { createWorkspaceLink, drawnWith } from './link'

function entryHolding(pane: string): Group {
  return {
    id: `group-${pane}`,
    panes: [{ id: pane, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function stateWith(activeGroup: number, focusCount: number): DrawnState {
  const workspace: Workspace = {
    groups: [entryHolding('a'), entryHolding('b')],
    activeGroup,
    tabGroups: []
  }
  return {
    revision: 5,
    workspace,
    agentRows: {},
    waiting: 'a',
    refocus: 2,
    focusCount,
    agents: {},
    defaultAgentName: 'Claude'
  }
}

function activeIdOf(state: DrawnState): string | undefined {
  if (state.workspace === null) {
    return undefined
  }
  return activePane(state.workspace)?.id
}

interface Pending {
  answer: (reply: RequestReply) => void
  fail: (reason: Error) => void
}

interface FakeWorkspace {
  api: TacoShellsApi['workspace']
  sent: WorkspaceRequest[]
  push(state: DrawnState): void
  answer(index: number, reply: RequestReply): void
  fail(index: number): void
}

function fakeWorkspace(initial: DrawnState): FakeWorkspace {
  const sent: WorkspaceRequest[] = []
  const pending: Pending[] = []
  let listener: (state: DrawnState) => void = () => undefined

  const api: TacoShellsApi['workspace'] = {
    initial,
    request: (request) => {
      sent.push(request)
      return new Promise((answer, fail) => {
        pending.push({ answer, fail })
      })
    },
    onState: (onState) => {
      listener = onState
      return () => undefined
    }
  }

  return {
    api,
    sent,
    push: (state) => {
      listener(state)
    },
    answer: (index, reply) => {
      pending[index].answer(reply)
    },
    fail: (index) => {
      pending[index].fail(new Error('the request did not arrive'))
    }
  }
}

function settled(): Promise<void> {
  return new Promise((resume) => {
    setTimeout(resume, 0)
  })
}

describe('drawnWith', () => {
  it("draws the workspace's state when the window is not ahead", () => {
    const state = stateWith(0, 1)

    const result = drawnWith(state, null)

    expect(result.drawn).toBe(state)
    expect(result.ahead).toBeNull()
  })

  it("keeps the window's focus move over a state that answers an older one", () => {
    const state = stateWith(0, 1)
    const ahead = { pane: 'b', count: 2 }

    const result = drawnWith(state, ahead)

    expect(activeIdOf(result.drawn)).toBe('b')
    expect(result.ahead).toBe(ahead)
    expect({ ...result.drawn, workspace: null }).toEqual({ ...state, workspace: null })
  })

  it('follows the workspace once it has applied the move', () => {
    const ahead = { pane: 'b', count: 2 }

    const applied = stateWith(1, 2)
    const appliedResult = drawnWith(applied, ahead)
    expect(appliedResult.drawn).toBe(applied)
    expect(appliedResult.ahead).toBeNull()

    const movedOn = stateWith(0, 2)
    const movedOnResult = drawnWith(movedOn, ahead)
    expect(movedOnResult.drawn).toBe(movedOn)
    expect(movedOnResult.ahead).toBeNull()
  })

  it('drops a move to a terminal that has gone', () => {
    const state = stateWith(0, 1)

    const result = drawnWith(state, { pane: 'gone', count: 2 })

    expect(result.drawn).toBe(state)
    expect(result.ahead).toBeNull()
  })
})

describe('createWorkspaceLink', () => {
  it('resolves a request only once the window holds the state with its effect', async () => {
    const workspace = fakeWorkspace(stateWith(0, 0))
    const link = createWorkspaceLink(workspace.api)
    const resolved: number[] = []

    void link.request({ type: 'tabGroupCreated', pane: 'a' }).then((reply) => {
      resolved.push(reply.revision)
    })
    workspace.answer(0, { ok: true, tabGroup: 'tab-group-1', revision: 6 })
    await settled()
    expect(resolved).toEqual([])

    workspace.push({ ...stateWith(0, 0), revision: 6 })
    await settled()
    expect(resolved).toEqual([6])

    void link.request({ type: 'tabGroupCreated', pane: 'a' }).then((reply) => {
      resolved.push(reply.revision)
    })
    workspace.answer(1, { ok: true, tabGroup: null, revision: 6 })
    await settled()
    expect(resolved).toEqual([6, 6])
  })

  it('ignores a state it already holds or has passed', () => {
    const initial = stateWith(0, 0)
    const workspace = fakeWorkspace(initial)
    const link = createWorkspaceLink(workspace.api)
    let redraws = 0
    link.subscribe(() => {
      redraws += 1
    })

    workspace.push({ ...stateWith(1, 0), revision: initial.revision })
    workspace.push({ ...stateWith(1, 0), revision: initial.revision - 1 })

    expect(redraws).toBe(0)
    expect(link.getDrawn()).toBe(initial)
  })

  it('draws a focus move at once, reports it with the next count, and asks nothing for the active terminal', () => {
    const workspace = fakeWorkspace(stateWith(0, 7))
    const link = createWorkspaceLink(workspace.api)

    link.focus('a')
    expect(workspace.sent).toEqual([])

    link.focus('b')
    expect(activeIdOf(link.getDrawn())).toBe('b')
    expect(workspace.sent).toEqual([{ type: 'focusReported', pane: 'b', count: 8 }])
  })

  it('lets go of a focus move the workspace never heard of', async () => {
    const workspace = fakeWorkspace(stateWith(0, 0))
    const link = createWorkspaceLink(workspace.api)

    link.focus('b')
    workspace.fail(0)
    await settled()

    expect(activeIdOf(link.getDrawn())).toBe('a')
  })
})
