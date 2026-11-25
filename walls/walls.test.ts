import { resolve } from 'node:path'

import { expect, test } from 'vitest'

import { WALLS } from './areas'
import { closed, unlisted, violationsIn } from './check'
import { HOLES } from './holes'
import { sourceFiles } from './tree'

const REPO_ROOT = resolve(import.meta.dirname, '..')

test('no import crosses a wall, except the holes still open', () => {
  const found = violationsIn(sourceFiles(REPO_ROOT), WALLS)
  expect(unlisted(found, HOLES)).toEqual([])
})

test('every listed hole is still open', () => {
  const found = violationsIn(sourceFiles(REPO_ROOT), WALLS)
  expect(closed(found, HOLES)).toEqual([])
})
