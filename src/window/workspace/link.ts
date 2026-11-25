import type { SessionId, TacoShellsApi } from '@protocol/ipc'
import type { DrawnState, RequestReply, WorkspaceRequest } from '@protocol/workspace'
import { activeSessionOf, locatePane } from '@rules/arrangement'
import { activatePane } from '@rules/focus'

/** A focus move the window has made and the workspace has not yet applied: the terminal, and which of the window's moves it was. */
export interface Ahead {
  pane: SessionId
  count: number
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

  // The workspace has caught up, so whichever terminal it says is active is the truth again.
  if (state.focusCount >= ahead.count) {
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

export function createWorkspaceLink(api: TacoShellsApi['workspace']): WorkspaceLink {
  let latest = api.initial
  let ahead: Ahead | null = null
  // Counting on from the workspace's count keeps a reloaded window's moves ahead of the ones it made before.
  let moves = latest.focusCount
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

    // Only a window can hold the keyboard, and a key typed straight after a focus move must
    // land in the new terminal. So the move is drawn here at once and reported afterwards.
    focus(pane) {
      const active = activeSessionOf(drawn.workspace)
      if (active === pane) {
        return
      }

      moves += 1
      const count = moves
      ahead = { pane, count }
      redraw()

      api.request({ type: 'focusReported', pane, count }).catch(() => {
        // The workspace never heard of this move, so nothing will ever answer it.
        if (ahead !== null && ahead.count === count) {
          ahead = null
          redraw()
        }
      })
    }
  }
}
