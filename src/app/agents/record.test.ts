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

  it('creates the config directory when it is not there yet', () => {
    const fresh = join(directory, 'not', 'yet')
    const record = { declined: true, written: [] }

    writeAgentRecord(fresh, 'claude', record)

    expect(readAgentRecord(fresh, 'claude')).toEqual(record)
  })

  it('reads a broken file as nothing recorded', () => {
    const nothing = { declined: false, written: [] }
    const file = join(directory, 'agents.json')

    expect(readAgentRecord(directory, 'claude')).toEqual(nothing)

    writeFileSync(file, 'not json')
    expect(readAgentRecord(directory, 'claude')).toEqual(nothing)

    const broken = [
      '{"claude": {"declined": "yes", "written": []}}',
      '{"claude": {"declined": true, "written": "Stop:x.sh"}}',
      '{"claude": {"declined": true, "written": [1]}}'
    ]
    for (const text of broken) {
      writeFileSync(file, text)
      expect(readAgentRecord(directory, 'claude')).toEqual(nothing)
    }

    writeFileSync(file, '{"claude": {"declined": true, "written": []}}')
    for (const inherited of ['toString', '__proto__', 'constructor']) {
      expect(readAgentRecord(directory, inherited)).toEqual(nothing)
    }
  })
})
