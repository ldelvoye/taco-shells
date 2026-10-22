import { readFileSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseSnapshot, type Snapshot } from '@shared/snapshot'

const SNAPSHOT_FILE = 'workspace.json'
const TEMPORARY_FILE = 'workspace.json.tmp'

/** The path of the workspace snapshot inside a config directory. */
export function snapshotPath(directory: string): string {
  return join(directory, SNAPSHOT_FILE)
}

/** The snapshot on disk, or null when it is missing, unreadable or malformed. */
export function readSnapshot(directory: string): Snapshot | null {
  try {
    const text = readFileSync(snapshotPath(directory), 'utf8')
    return parseSnapshot(text)
  } catch {
    return null
  }
}

/** The snapshot's atomic write to disk; every failure is swallowed. */
export function writeSnapshot(directory: string, snapshot: Snapshot): void {
  try {
    const temporaryPath = join(directory, TEMPORARY_FILE)
    const text = JSON.stringify(snapshot)
    writeFileSync(temporaryPath, text)
    renameSync(temporaryPath, snapshotPath(directory))
  } catch {
    return
  }
}
