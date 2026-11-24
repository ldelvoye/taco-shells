import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { readAgentRecord, writeAgentRecord } from './record'

describe('agent record', () => {
  let directory: string

  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), 'taco-shells-record-'))
  })

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true })
  })

  it("keeps each agent's record apart", () => {
    const claude = { declined: true, written: ['SessionStart', 'SessionEnd'] }
    const other = { declined: false, written: ['Stop'] }

    writeAgentRecord(directory, 'claude', claude)
    writeAgentRecord(directory, 'other', other)

    expect(readAgentRecord(directory, 'claude')).toEqual(claude)
    expect(readAgentRecord(directory, 'other')).toEqual(other)
  })

  it('reads a broken file as nothing recorded', () => {
    const nothing = { declined: false, written: [] }
    const file = join(directory, 'agents.json')

    expect(readAgentRecord(directory, 'claude')).toEqual(nothing)

    writeFileSync(file, 'not json')
    expect(readAgentRecord(directory, 'claude')).toEqual(nothing)

    writeFileSync(file, '{"claude": {"declined": "yes", "written": []}}')
    expect(readAgentRecord(directory, 'claude')).toEqual(nothing)
  })
})
