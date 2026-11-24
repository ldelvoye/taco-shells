import { describe, expect, it } from 'vitest'

import { createWorkspaceStore } from './store'

const PANE = 'p1'
const SESSION = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'

describe('workspace store', () => {
  it('tells its listeners only about changes the window draws', () => {
    const store = createWorkspaceStore()
    let calls = 0
    store.subscribe(() => {
      calls += 1
    })

    store.dispatch({
      type: 'terminalOpened',
      group: {
        id: 'g1',
        panes: [{ id: PANE, title: '', width: 1, color: null }],
        activePane: 0,
        tabGroup: null
      }
    })
    expect(calls).toBe(1)

    store.dispatch({
      type: 'agentSignalled',
      signal: { id: PANE, harness: 'claude', sessionId: SESSION, signal: 'start' }
    })
    store.dispatch({ type: 'titleChanged', pane: PANE, title: '· Fix bug' })
    const callsBefore = calls
    const drawnBefore = store.getDrawnState()

    store.dispatch({ type: 'titleChanged', pane: PANE, title: '✶ Fix bug' })
    expect(calls).toBe(callsBefore)
    expect(store.getDrawnState()).toBe(drawnBefore)
    expect(store.getState().rawTitles.get(PANE)).toBe('✶ Fix bug')

    store.dispatch({ type: 'titleChanged', pane: PANE, title: '✶ Ship it' })
    expect(calls).toBe(callsBefore + 1)
    expect(store.getDrawnState()).toBe(store.getState())
  })
})
