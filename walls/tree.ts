import { readFileSync, readdirSync } from 'node:fs'
import { join, relative, sep } from 'node:path'

const SOURCE_FILE = /\.tsx?$/

function walk(folder: string, found: string[]): void {
  const entries = readdirSync(folder, { withFileTypes: true })
  for (const entry of entries) {
    const path = join(folder, entry.name)
    if (entry.isDirectory()) {
      walk(path, found)
    } else if (SOURCE_FILE.test(entry.name)) {
      found.push(path)
    }
  }
}

export function sourceFiles(repoRoot: string): Map<string, string> {
  const paths: string[] = []
  walk(join(repoRoot, 'src'), paths)

  const files = new Map<string, string>()
  for (const path of paths) {
    const repoRelative = relative(repoRoot, path)
    const key = repoRelative.split(sep).join('/')
    files.set(key, readFileSync(path, 'utf8'))
  }
  return files
}
