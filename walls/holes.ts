import type { Hole } from './check'

/** The steps of the core rip out that close holes, by the number a hole's `closes` names. */
export const CLOSING_MILESTONES: Record<number, string> = {}

// The imports that cross a wall today. This list only shrinks: closing a hole means deleting its line, and the test fails on a line whose hole is gone.
export const HOLES: Hole[] = []
