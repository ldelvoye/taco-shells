import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const RECORD_FILE = 'agents.json'

/** What the app remembers about one agent's setup: a decline, and the entries it wrote. */
export interface AgentRecord {
  declined: boolean
  written: string[]
}

type RecordFile = Record<string, unknown>

function emptyRecord(): AgentRecord {
  return { declined: false, written: [] }
}

function isPlainObject(value: unknown): value is RecordFile {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isStringArray(value: unknown): value is string[] {
  if (!Array.isArray(value)) {
    return false
  }
  return value.every((item) => typeof item === 'string')
}

function readRecordFile(configDirectory: string): RecordFile {
  const path = join(configDirectory, RECORD_FILE)
  let parsed: unknown
  try {
    const text = readFileSync(path, 'utf8')
    parsed = JSON.parse(text)
  } catch {
    return {}
  }
  if (!isPlainObject(parsed)) {
    return {}
  }
  return parsed
}

export function readAgentRecord(configDirectory: string, harnessId: string): AgentRecord {
  const file = readRecordFile(configDirectory)
  const entry = file[harnessId]
  if (!isPlainObject(entry)) {
    return emptyRecord()
  }
  if (typeof entry.declined !== 'boolean' || !isStringArray(entry.written)) {
    return emptyRecord()
  }
  return { declined: entry.declined, written: entry.written }
}

export function writeAgentRecord(configDirectory: string, harnessId: string, record: AgentRecord): void {
  const file = readRecordFile(configDirectory)
  file[harnessId] = record

  mkdirSync(configDirectory, { recursive: true })
  const path = join(configDirectory, RECORD_FILE)
  const text = JSON.stringify(file, null, 2)
  writeFileSync(path, `${text}\n`)
}
