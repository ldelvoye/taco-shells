import { describe, expect, it } from 'vitest'

import type { Group, Workspace } from '@protocol/arrangement'
import type { DrawnState } from '@protocol/workspace'
import { activePane } from '@rules/arrangement'
import { drawnWith } from './link'

function entryHolding(pane: string): Group {
  return {
    id: `group-${pane}`,
    panes: [{ id: pane, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function stateWith(activeGroup: number, focusMove: number): DrawnState {
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
    focusMove,
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

describe('drawnWith', () => {
  it("draws the workspace's state when the window is not ahead", () => {
    const state = stateWith(0, 1)

    const result = drawnWith(state, null)

    expect(result.drawn).toBe(state)
    expect(result.ahead).toBeNull()
  })

  it("keeps the window's focus move over a state that answers an older one", () => {
    const state = stateWith(0, 1)
    const ahead = { pane: 'b', move: 2 }

    const result = drawnWith(state, ahead)

    expect(activeIdOf(result.drawn)).toBe('b')
    expect(result.ahead).toBe(ahead)
    expect({ ...result.drawn, workspace: null }).toEqual({ ...state, workspace: null })
  })

  it('follows the workspace once it has applied the move', () => {
    const ahead = { pane: 'b', move: 2 }

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

    const result = drawnWith(state, { pane: 'gone', move: 2 })

    expect(result.drawn).toBe(state)
    expect(result.ahead).toBeNull()
  })
})
