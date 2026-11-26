import { resolve } from 'node:path'

import { expect, test } from 'vitest'

import { WALLS } from './areas'
import { violationsIn } from './check'
import { sourceFiles } from './tree'

const REPO_ROOT = resolve(import.meta.dirname, '..')

const FILES = sourceFiles(REPO_ROOT)
const FOUND = violationsIn(FILES, WALLS)

test('no import crosses a wall', () => {
  expect(FOUND).toEqual([])
})
