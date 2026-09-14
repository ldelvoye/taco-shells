import { execFileSync } from 'node:child_process'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { beforeAll, describe, expect, it } from 'vitest'

const SCRIPTS = dirname(fileURLToPath(import.meta.url))

let repository

function git(...args) {
  execFileSync('git', args, { cwd: repository, stdio: 'ignore' })
}

function commit(message) {
  writeFileSync(join(repository, 'file'), message)
  git('add', 'file')
  git('commit', '-m', message)
}

/**
 * Runs the real pipeline inside the scratch repository. A child process is what
 * lets the module read that repository rather than this one, without the
 * production code carrying a directory argument it would never otherwise need.
 */
function bumpAndNotes() {
  const script = `
    import { changesSince, lastTag, messagesSince } from ${JSON.stringify(join(SCRIPTS, 'repo.mjs'))}
    import { bumpFor } from ${JSON.stringify(join(SCRIPTS, 'version.mjs'))}
    const previous = lastTag()
    console.log(JSON.stringify({
      bump: bumpFor(messagesSince(previous)),
      notes: changesSince(previous)
    }))
  `
  const output = execFileSync('node', ['--input-type=module', '-e', script], {
    cwd: repository,
    encoding: 'utf8'
  })
  return JSON.parse(output)
}

describe('choosing a version from real commits', () => {
  beforeAll(() => {
    repository = mkdtempSync(join(tmpdir(), 'taco-shells-repo-'))
    git('init', '--initial-branch', 'main')
    git('config', 'user.email', 'test@example.com')
    git('config', 'user.name', 'Test')
    commit('feat: the first thing')
    git('tag', 'v0.1.0')
  })

  // The bump reads commit bodies and the notes read subjects, and they are two
  // different git invocations. Wiring the notes into the bump would silently
  // release a breaking change as a patch, which is what this pins.
  it('sees a breaking change declared in a commit body', () => {
    commit('fix: a small tweak\n\nBREAKING CHANGE: the config format moved')

    const { bump, notes } = bumpAndNotes()

    expect(bump).toBe('major')
    expect(notes).toEqual(['fix: a small tweak'])
  })
})
