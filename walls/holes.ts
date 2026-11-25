import type { Hole } from './check'

/** The steps of the core rip out that close holes, by the number a hole's `closes` names. */
export const CLOSING_MILESTONES: Record<number, string> = {
  3: 'the workspace takes the arrangement: the window stops running the reducer',
  4: 'the workspace opens and closes terminals, through messages to the runtime',
  5: 'the runtime gets its own process'
}

// The imports that cross a wall today. This list only shrinks: closing a hole means deleting its line, and the test fails on a line whose hole is gone.
export const HOLES: Hole[] = [
  // The window runs the workspace reducer, its state and events.
  { file: 'src/window/workspace/operations.ts', imports: '@workspace/workspaceState', closes: 3 },
  { file: 'src/window/workspace/store.ts', imports: '@workspace/workspaceState', closes: 3 },
  { file: 'src/window/workspace/useWorkspace.ts', imports: '@workspace/workspaceState', closes: 3 },

  // Restore runs in the window.
  { file: 'src/window/workspace/operations.ts', imports: '@workspace/snapshot/format', closes: 4 },

  // The window reads the agent registry.
  { file: 'src/window/sidebar/Sidebar.tsx', imports: '@agents/registry', closes: 3 },

  // App services import the runtime.
  { file: 'src/app/agents/index.ts', imports: '@runtime/foreground', closes: 4 },
  { file: 'src/app/agents/index.ts', imports: '@runtime/sessions', closes: 4 },
  { file: 'src/app/focus/index.ts', imports: '@runtime/sessions', closes: 4 },

  // The app host imports the runtime.
  { file: 'src/app/host/index.ts', imports: '@runtime/sessions', closes: 5 },
  { file: 'src/app/host/ipc/index.ts', imports: '@runtime/sessions', closes: 5 },

  // The workspace imports the runtime.
  { file: 'src/workspace/snapshot/store.ts', imports: '@runtime/cwd', closes: 4 },
  { file: 'src/workspace/snapshot/store.ts', imports: '@runtime/sessions', closes: 4 },

  // The runtime imports the workspace.
  { file: 'src/runtime/sessions.ts', imports: '@workspace/startCommand', closes: 4 }
]
