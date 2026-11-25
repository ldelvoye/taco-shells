import type { Hole } from './check'

/** The steps of the core rip out that close holes, by the number a hole's `closes` names. */
export const CLOSING_MILESTONES: Record<number, string> = {
  4: 'the workspace opens and closes terminals, through messages to the runtime',
  5: 'the runtime gets its own process'
}

// The imports that cross a wall today. This list only shrinks: closing a hole means deleting its line, and the test fails on a line whose hole is gone.
export const HOLES: Hole[] = [
  // The app host imports the runtime.
  { file: 'src/app/host/index.ts', imports: '@runtime/sessions', closes: 5 },
  { file: 'src/app/host/ipc/index.ts', imports: '@runtime/sessions', closes: 5 }
]
