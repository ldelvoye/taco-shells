import { resolve } from 'node:path'

import { expect, test } from 'vitest'

import { WALLS } from './areas'
import { closed, unlisted, violationsIn } from './check'
import { CLOSING_MILESTONES, HOLES } from './holes'
import { sourceFiles } from './tree'

const REPO_ROOT = resolve(import.meta.dirname, '..')

const FILES = sourceFiles(REPO_ROOT)
const FOUND = violationsIn(FILES, WALLS)

test('no import crosses a wall, except the holes still open', () => {
  expect(unlisted(FOUND, HOLES)).toEqual([])
})

test('every listed hole is still open', () => {
  expect(closed(FOUND, HOLES)).toEqual([])
})

test('every hole names a milestone that closes it', () => {
  const unnamed = HOLES.filter((hole) => !(hole.closes in CLOSING_MILESTONES))
  expect(unnamed).toEqual([])
})
