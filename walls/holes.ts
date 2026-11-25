import type { Hole } from './check'

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

  // App services import electron.
  { file: 'src/app/agents/index.ts', imports: 'electron', closes: 2 },
  { file: 'src/app/config/index.ts', imports: 'electron', closes: 2 },
  { file: 'src/app/focus/index.ts', imports: 'electron', closes: 2 },
  { file: 'src/app/update/index.ts', imports: 'electron', closes: 2 },

  // App services import the window sender and the menus from the host.
  { file: 'src/app/agents/index.ts', imports: '../host/window/applicationMenu', closes: 2 },
  { file: 'src/app/agents/index.ts', imports: '../host/window/send', closes: 2 },
  { file: 'src/app/agents/setups.ts', imports: '../host/window/menu', closes: 2 },
  { file: 'src/app/focus/index.ts', imports: '../host/window/send', closes: 2 },
  { file: 'src/app/update/index.ts', imports: '../host/window/applicationMenu', closes: 2 },
  { file: 'src/app/update/index.ts', imports: '../host/window/send', closes: 2 },

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
