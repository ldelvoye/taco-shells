import type { SessionId, TacoShellsApi } from '@protocol/ipc'
import type { DrawnState, RequestReply, WorkspaceRequest } from '@protocol/workspace'
import { activePane, locatePane } from '@rules/arrangement'
import { activatePane } from '@rules/focus'

export interface Ahead {
  pane: SessionId
  move: number
}

interface Waiter {
  revision: number
  resume: () => void
}

export interface WorkspaceLink {
  getDrawn(): DrawnState
  subscribe(listener: () => void): () => void
  request(request: WorkspaceRequest): Promise<RequestReply>
  focus(pane: SessionId): void
}

/** What to draw: the workspace's state, with the window's own focus move laid over it until the workspace has applied that move. */
export function drawnWith(
  state: DrawnState,
  ahead: Ahead | null
): { drawn: DrawnState; ahead: Ahead | null } {
  if (ahead === null) {
    return { drawn: state, ahead: null }
  }

  if (state.focusMove >= ahead.move) {
    return { drawn: state, ahead: null }
  }

  const arrangement = state.workspace
  if (arrangement === null) {
    return { drawn: state, ahead: null }
  }

  const location = locatePane(arrangement, ahead.pane)
  if (location === null) {
    return { drawn: state, ahead: null }
  }

  const moved = activatePane(arrangement, ahead.pane)
  const drawn: DrawnState = { ...state, workspace: moved }
  return { drawn, ahead }
}

function isActive(state: DrawnState, pane: SessionId): boolean {
  if (state.workspace === null) {
    return false
  }
  const active = activePane(state.workspace)
  if (active === null) {
    return false
  }
  return active.id === pane
}

export function createWorkspaceLink(api: TacoShellsApi['workspace']): WorkspaceLink {
  let latest = api.initial
  let ahead: Ahead | null = null
  let moves = 0
  let drawn = latest
  let waiters: Waiter[] = []
  const listeners = new Set<() => void>()

  function redraw(): void {
    const result = drawnWith(latest, ahead)
    drawn = result.drawn
    ahead = result.ahead
    for (const listener of listeners) {
      listener()
    }
  }

  api.onState((state) => {
    if (state.revision <= latest.revision) {
      return
    }
    latest = state
    redraw()

    const reached = waiters.filter((waiter) => waiter.revision <= state.revision)
    waiters = waiters.filter((waiter) => waiter.revision > state.revision)
    for (const waiter of reached) {
      waiter.resume()
    }
  })

  // A reply can overtake the state that holds its effect, and whoever asked acts on that state.
  function caughtUpTo(revision: number): Promise<void> {
    if (latest.revision >= revision) {
      return Promise.resolve()
    }
    return new Promise((resume) => {
      waiters.push({ revision, resume })
    })
  }

  return {
    getDrawn() {
      return drawn
    },

    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },

    async request(request) {
      const reply = await api.request(request)
      await caughtUpTo(reply.revision)
      return reply
    },

    focus(pane) {
      if (isActive(drawn, pane)) {
        return
      }

      moves += 1
      ahead = { pane, move: moves }
      redraw()
      void api.request({ type: 'focusReported', pane, move: moves }).catch(() => undefined)
    }
  }
}
