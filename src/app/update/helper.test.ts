import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

const HELPER = join(import.meta.dirname, '..', '..', '..', 'resources', 'update-helper.sh')
// Stands in for macOS's open: it records what it was asked to open and refuses an app holding a "broken" file.
const OPEN_STUB = '#!/bin/sh\nprintf \'%s\\n\' "$1" >> "$OPEN_LOG"\nif [ -e "$1/broken" ]; then exit 1; fi\n'

let scratch: string
let installed: string
let extracted: string
let openLog: string

function makeApp(path: string, marker: string): void {
  mkdirSync(path, { recursive: true })
  writeFileSync(join(path, marker), '')
}

// The helper moves the old app into the Trash under HOME, which is the scratch folder here.
function makeTrash(): void {
  mkdirSync(join(scratch, '.Trash'))
}

function runHelper(): void {
  const stubDirectory = join(scratch, 'bin')
  mkdirSync(stubDirectory)
  writeFileSync(join(stubDirectory, 'open'), OPEN_STUB, { mode: 0o755 })

  const appThatHasQuit = spawnSync('true')
  const env = { HOME: scratch, OPEN_LOG: openLog, PATH: `${stubDirectory}:/bin:/usr/bin` }
  spawnSync('/bin/sh', [HELPER, String(appThatHasQuit.pid), installed, extracted], { env })
}

function opened(): string[] {
  const log = readFileSync(openLog, 'utf8')
  const lines = log.split('\n')
  return lines.filter((line) => line !== '')
}

beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), 'taco-update-helper-'))
  installed = join(scratch, 'Applications', 'Fake.app')
  extracted = join(scratch, 'updates', 'Fake.app')
  openLog = join(scratch, 'opened')
  makeApp(installed, 'old')
})

afterEach(() => {
  rmSync(scratch, { recursive: true, force: true })
})

describe('the update helper', () => {
  it('reopens the installed app when there is no Trash to move it to', () => {
    makeApp(extracted, 'new')

    runHelper()

    expect(existsSync(join(installed, 'old'))).toBe(true)
    expect(opened()).toEqual([installed])
  })

  it('puts the old app back and reopens it when the new one is missing', () => {
    makeTrash()

    runHelper()

    expect(existsSync(join(installed, 'old'))).toBe(true)
    expect(opened()).toEqual([installed])
  })

  it('puts the old app back and reopens it when the new one will not open', () => {
    makeTrash()
    makeApp(extracted, 'new')
    writeFileSync(join(extracted, 'broken'), '')

    runHelper()

    const newAppThenOldApp = [installed, installed]
    expect(existsSync(join(installed, 'old'))).toBe(true)
    expect(existsSync(join(extracted, 'new'))).toBe(true)
    expect(opened()).toEqual(newAppThenOldApp)
  })
})
