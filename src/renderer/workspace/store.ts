import {
  drawsTheSame,
  initialWorkspaceState,
  reduceWorkspaceState,
  type WorkspaceEvent,
  type WorkspaceState
} from '@shared/workspaceState'

export interface WorkspaceStore {
  getState(): WorkspaceState
  getDrawnState(): WorkspaceState
  dispatch(event: WorkspaceEvent): void
  subscribe(listener: () => void): () => void
}

export function createWorkspaceStore(): WorkspaceStore {
  let state = initialWorkspaceState()
  let drawn = state
  const listeners = new Set<() => void>()

  return {
    getState() {
      return state
    },

    getDrawnState() {
      return drawn
    },

    dispatch(event) {
      const next = reduceWorkspaceState(state, event)
      state = next
      if (drawsTheSame(drawn, next)) {
        return
      }

      drawn = next
      for (const listener of listeners) {
        listener()
      }
    },

    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    }
  }
}
