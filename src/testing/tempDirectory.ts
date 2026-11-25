import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const created: string[] = []

/** A new temp directory, removed by the next removeTempDirectories. */
export function makeTempDirectory(prefix: string): string {
  const template = join(tmpdir(), prefix)
  const directory = mkdtempSync(template)
  created.push(directory)
  return directory
}

export function removeTempDirectories(): void {
  const directories = created.splice(0)
  for (const directory of directories) {
    rmSync(directory, { recursive: true, force: true })
  }
}
