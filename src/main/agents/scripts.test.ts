import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { HARNESSES } from '@shared/harness/registry'
import { makeTempDirectory, removeTempDirectories } from '../testing/tempDirectory'
import { copyHookScripts, hookScriptsSource } from './scripts'

const REPO_ROOT = join(import.meta.dirname, '..', '..', '..')

afterEach(removeTempDirectories)

function makeSource(): string {
  const root = makeTempDirectory('taco-shells-scripts-')
  const source = join(root, 'source')
  mkdirSync(source)
  writeFileSync(join(source, 'one.sh'), 'echo one\n', { mode: 0o644 })
  writeFileSync(join(source, 'two.sh'), 'echo two\n', { mode: 0o644 })
  return source
}

function destinationFor(source: string): string {
  return join(source, '..', 'config', 'hooks')
}

describe('the hook entries harnesses declare', () => {
  it('name scripts the repo ships, with no entry listed twice', () => {
    const shipped = hookScriptsSource(false, '', REPO_ROOT)

    for (const harness of HARNESSES) {
      if (harness.setup === undefined) {
        continue
      }
      const entries = harness.setup.entries
      const missing = entries.filter((entry) => !existsSync(join(shipped, entry.script)))
      expect(missing).toEqual([])

      const pairs = entries.map((entry) => `${entry.event} ${entry.script}`)
      const distinctPairs = new Set(pairs)
      expect(distinctPairs.size).toBe(pairs.length)
    }
  })
})

describe('copyHookScripts', () => {
  it('copies every script and makes it runnable', () => {
    const source = makeSource()
    const destination = destinationFor(source)
    mkdirSync(join(source, 'not-a-script'))

    const problems = copyHookScripts(source, destination)

    expect(problems).toEqual([])
    expect(existsSync(join(destination, 'not-a-script'))).toBe(false)
    for (const name of ['one.sh', 'two.sh']) {
      const stats = statSync(join(destination, name))
      expect(stats.mode & 0o111).not.toBe(0)
    }
  })

  it('restores a script that was changed', () => {
    const source = makeSource()
    const destination = destinationFor(source)
    copyHookScripts(source, destination)
    writeFileSync(join(destination, 'one.sh'), 'echo tampered\n')

    copyHookScripts(source, destination)

    const restored = readFileSync(join(destination, 'one.sh'), 'utf8')
    expect(restored).toBe('echo one\n')
  })

  it('reports a missing source instead of throwing', () => {
    const source = makeSource()
    const missing = join(source, 'nowhere')
    const destination = destinationFor(source)

    const problems = copyHookScripts(missing, destination)

    expect(problems).toHaveLength(1)
  })
})
