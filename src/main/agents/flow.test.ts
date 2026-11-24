import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { claude } from '@shared/harness/claude'
import type { HarnessSetup } from '@shared/harness/types'
import {
  agentPathsOf,
  canWriteFor,
  decline,
  keepCurrent,
  readState,
  removeSetup,
  setUp,
  switchToManaged,
  type Agent
} from './flow'
import { readAgentRecord } from './record'

const MISSING_HOME = '/nonexistent-home'
const SESSION_SCRIPT = 'taco-shells-claude-session.sh'
const ATTENTION_SCRIPT = 'taco-shells-attention.sh'

const setup = claude.setup as HarnessSetup

let appDirectory = ''
let agentDirectory = ''
let extraDirectory = ''

function agentFor(directory: string): Agent {
  const env = { CLAUDE_CONFIG_DIR: directory }
  const paths = agentPathsOf(setup, appDirectory, env, MISSING_HOME)
  return { id: claude.id, setup, paths }
}

function writeJson(path: string, value: unknown): void {
  writeFileSync(path, JSON.stringify(value, null, 2))
}

function readJson(path: string): Record<string, unknown> {
  const text = readFileSync(path, 'utf8')
  return JSON.parse(text) as Record<string, unknown>
}

function commandsIn(path: string): string[] {
  const settings = readJson(path)
  const hooks = settings.hooks as Record<string, { hooks: { command: string }[] }[]>
  const groups = Object.values(hooks).flat()
  const entries = groups.flatMap((group) => group.hooks)
  return entries.map((entry) => entry.command)
}

function userHook(command: string): { hooks: { type: string; command: string }[] } {
  return { hooks: [{ type: 'command', command }] }
}

beforeEach(() => {
  appDirectory = mkdtempSync(join(tmpdir(), 'taco-shells-flow-'))
  agentDirectory = mkdtempSync(join(tmpdir(), 'taco-shells-flow-'))
  extraDirectory = mkdtempSync(join(tmpdir(), 'taco-shells-flow-'))
})

afterEach(() => {
  rmSync(appDirectory, { recursive: true, force: true })
  rmSync(agentDirectory, { recursive: true, force: true })
  rmSync(extraDirectory, { recursive: true, force: true })
})

describe('the agent setup flow', () => {
  it('sets up, then reads as set up', () => {
    const agent = agentFor(agentDirectory)
    writeFileSync(agent.paths.settingsPath, '{}')
    expect(readState(agent, true)).toBe('notSetUp')

    expect(setUp(agent)).toBeNull()

    const commands = commandsIn(agent.paths.settingsPath)
    expect(commands).toHaveLength(10)
    expect(commands.every((command) => command.startsWith(agent.paths.hooksDirectory))).toBe(true)
    const record = readAgentRecord(appDirectory, claude.id)
    expect(record.declined).toBe(false)
    expect(record.written).toHaveLength(10)
    expect(readState(agent, true)).toBe('setUp')

    const missing = agentFor(join(extraDirectory, 'absent'))
    expect(readState(missing, true)).toBe('notInstalled')
  })

  it('leaves a deleted entry deleted', () => {
    const agent = agentFor(agentDirectory)
    writeFileSync(agent.paths.settingsPath, '{}')
    setUp(agent)
    const settings = readJson(agent.paths.settingsPath)
    const hooks = settings.hooks as Record<string, unknown>
    delete hooks.StopFailure
    writeJson(agent.paths.settingsPath, settings)

    expect(keepCurrent(agent)).toBeNull()

    const after = readJson(agent.paths.settingsPath)
    expect(Object.keys(after.hooks as Record<string, unknown>)).not.toContain('StopFailure')
  })

  it('adds an entry a newer version introduced', () => {
    const agent = agentFor(agentDirectory)
    writeFileSync(agent.paths.settingsPath, '{}')
    setUp(agent)
    const settings = readJson(agent.paths.settingsPath)
    const hooks = settings.hooks as Record<string, unknown>
    delete hooks.StopFailure
    writeJson(agent.paths.settingsPath, settings)
    const record = readAgentRecord(appDirectory, claude.id)
    const written = record.written.filter((key) => !key.startsWith('StopFailure:'))
    writeJson(join(appDirectory, 'agents.json'), { [claude.id]: { declined: false, written } })

    expect(keepCurrent(agent)).toBeNull()

    expect(commandsIn(agent.paths.settingsPath)).toHaveLength(10)
    expect(readAgentRecord(appDirectory, claude.id).written).toHaveLength(10)
  })

  it('removes its entries and counts as a decline', () => {
    const agent = agentFor(agentDirectory)
    const mine = '/Users/x/bin/my-hook.sh'
    writeJson(agent.paths.settingsPath, { hooks: { Stop: [userHook(mine)] } })
    setUp(agent)
    expect(commandsIn(agent.paths.settingsPath)).toHaveLength(11)

    expect(removeSetup(agent)).toBeNull()

    expect(commandsIn(agent.paths.settingsPath)).toEqual([mine])
    expect(readAgentRecord(appDirectory, claude.id)).toEqual({ declined: true, written: [] })
    expect(readState(agent, true)).toBe('declined')

    expect(setUp(agent)).toBeNull()
    expect(readState(agent, true)).toBe('setUp')
  })

  it('switches from a clone to managed', () => {
    const agent = agentFor(agentDirectory)
    const cloneDirectory = '/Users/x/code/taco-shells/hooks'
    const mine = '/Users/x/bin/my-hook.sh'
    writeJson(agent.paths.settingsPath, {
      hooks: {
        SessionStart: [userHook(`${cloneDirectory}/${SESSION_SCRIPT}`)],
        Stop: [userHook(`${cloneDirectory}/${ATTENTION_SCRIPT}`), userHook(mine)]
      }
    })
    expect(readState(agent, true)).toBe('byHand')

    expect(switchToManaged(agent)).toBeNull()

    const commands = commandsIn(agent.paths.settingsPath)
    expect(commands.some((command) => command.includes('/code/taco-shells/hooks/'))).toBe(false)
    const managed = commands.filter((command) => command.startsWith(agent.paths.hooksDirectory))
    expect(managed).toHaveLength(10)
    expect(commands).toContain(mine)
    expect(readState(agent, true)).toBe('setUp')
  })

  it('changes nothing when the file cannot be read as JSON', () => {
    const agent = agentFor(agentDirectory)
    const broken = '{ not json'
    writeFileSync(agent.paths.settingsPath, broken)

    expect(setUp(agent)).not.toBeNull()
    expect(removeSetup(agent)).not.toBeNull()
    expect(switchToManaged(agent)).not.toBeNull()

    expect(readFileSync(agent.paths.settingsPath, 'utf8')).toBe(broken)
    expect(existsSync(join(appDirectory, 'agents.json'))).toBe(false)
  })

  it('writes through a symlinked settings file', () => {
    const agent = agentFor(agentDirectory)
    const realPath = join(extraDirectory, 'settings.json')
    writeFileSync(realPath, '{}')
    symlinkSync(realPath, agent.paths.settingsPath)

    expect(setUp(agent)).toBeNull()

    expect(lstatSync(agent.paths.settingsPath).isSymbolicLink()).toBe(true)
    expect(commandsIn(realPath)).toHaveLength(10)
  })

  it('remembers a decline without touching the entries it wrote', () => {
    const agent = agentFor(agentDirectory)
    mkdirSync(agentDirectory, { recursive: true })
    writeFileSync(agent.paths.settingsPath, '{}')
    setUp(agent)

    expect(decline(agent)).toBeNull()

    const record = readAgentRecord(appDirectory, claude.id)
    expect(record.declined).toBe(true)
    expect(record.written).toHaveLength(10)
    expect(readState(agent, true)).toBe('setUp')
  })

  it('may write in a development build only with both directories overridden', () => {
    const app = 'TACO_SHELLS_CONFIG_DIR'
    const agentVariable = 'CLAUDE_CONFIG_DIR'
    expect(canWriteFor(setup, false, {})).toBe(false)
    expect(canWriteFor(setup, false, { [app]: '/a' })).toBe(false)
    expect(canWriteFor(setup, false, { [agentVariable]: '/b' })).toBe(false)
    expect(canWriteFor(setup, false, { [app]: '/a', [agentVariable]: '/b' })).toBe(true)
    expect(canWriteFor(setup, false, { [app]: '', [agentVariable]: '/b' })).toBe(false)
    expect(canWriteFor(setup, true, {})).toBe(true)
  })
})
