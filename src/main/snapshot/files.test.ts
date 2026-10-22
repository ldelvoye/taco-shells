import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { SNAPSHOT_VERSION, type Snapshot } from '@shared/snapshot'
import { readSnapshot, snapshotPath, writeSnapshot } from './files'

const SNAPSHOT: Snapshot = {
  version: SNAPSHOT_VERSION,
  groups: [{ panes: [{ width: 1, cwd: '/tmp', claude: null }], activePane: 0 }],
  activeGroup: 0
}

describe('snapshot files', () => {
  let directory: string

  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), 'taco-snapshot-'))
  })

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true })
  })

  it('reads nothing from a missing or malformed file', () => {
    expect(readSnapshot(directory)).toBeNull()

    writeFileSync(snapshotPath(directory), '{nope')
    expect(readSnapshot(directory)).toBeNull()
  })

  it('writes a snapshot that reads back and leaves no temporary file', () => {
    writeSnapshot(directory, SNAPSHOT)

    expect(readSnapshot(directory)).toEqual(SNAPSHOT)
    const temporary = join(directory, 'workspace.json.tmp')
    expect(existsSync(temporary)).toBe(false)
  })

  it('swallows a write into a path that is not a directory', () => {
    const notDirectory = join(directory, 'file')
    writeFileSync(notDirectory, '')

    expect(() => writeSnapshot(notDirectory, SNAPSHOT)).not.toThrow()
  })
})
