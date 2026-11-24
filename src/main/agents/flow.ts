import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { agentSetupStateOf, type AgentSetupState } from '@shared/agentSetup'
import type { Harness, HarnessSetup } from '@shared/harness/types'
import { errorMessage } from '../errorMessage'
import {
  addEntries,
  entryKey,
  hookWiringOf,
  removeHandEntries,
  removeManagedEntries,
  type HookSettingsResult
} from './hookSettings'
import { readAgentRecord, writeAgentRecord, type AgentRecord } from './record'
import { hooksDirectory } from './scripts'

const RECORD_FILE_NAME = 'agents.json'
const DEV_CONFIG_DIRECTORY_VARIABLE = 'TACO_SHELLS_CONFIG_DIR'

/** Where one agent's files and the app's own files for it live. */
export interface AgentPaths {
  configDirectory: string
  hooksDirectory: string
  agentDirectory: string
  settingsPath: string
}

/** One harness that declares a setup, with where its files are. */
export interface Agent {
  id: string
  setup: HarnessSetup
  paths: AgentPaths
}

interface SettingsRead {
  text: string | null
  problem: string | null
}

function isNotFound(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) {
    return false
  }
  const code = (error as { code?: unknown }).code
  return code === 'ENOENT'
}

function isSet(value: string | undefined): value is string {
  return typeof value === 'string' && value !== ''
}

function readSettings(path: string): SettingsRead {
  try {
    const text = readFileSync(path, 'utf8')
    return { text, problem: null }
  } catch (error) {
    if (isNotFound(error)) {
      return { text: null, problem: null }
    }
    const message = errorMessage(error)
    return { text: null, problem: `${path} could not be read: ${message}` }
  }
}

function writeSettings(path: string, text: string): string | null {
  try {
    writeFileSync(path, text)
  } catch (error) {
    const message = errorMessage(error)
    return `${path} could not be written: ${message}`
  }
  return null
}

function saveRecord(agent: Agent, record: AgentRecord): string | null {
  const configDirectory = agent.paths.configDirectory
  try {
    writeAgentRecord(configDirectory, agent.id, record)
  } catch (error) {
    const path = join(configDirectory, RECORD_FILE_NAME)
    const message = errorMessage(error)
    return `${path} could not be written: ${message}`
  }
  return null
}

function writeChanged(agent: Agent, result: HookSettingsResult): string | null {
  if (result.text === null) {
    return null
  }
  return writeSettings(agent.paths.settingsPath, result.text)
}

function saveSetUpRecord(agent: Agent): string | null {
  const written = agent.setup.entries.map(entryKey)
  return saveRecord(agent, { declined: false, written })
}

export function agentPathsOf(
  setup: HarnessSetup,
  configDirectory: string,
  env: NodeJS.ProcessEnv,
  home: string
): AgentPaths {
  const override = env[setup.configDirectoryVariable]
  let agentDirectory: string
  if (isSet(override)) {
    agentDirectory = override
  } else {
    agentDirectory = join(home, setup.defaultConfigDirectory)
  }
  const settingsPath = join(agentDirectory, setup.settingsFile)
  return {
    configDirectory,
    hooksDirectory: hooksDirectory(configDirectory),
    agentDirectory,
    settingsPath
  }
}

export function agentsOf(
  harnesses: readonly Harness[],
  configDirectory: string,
  env: NodeJS.ProcessEnv,
  home: string
): Agent[] {
  const agents: Agent[] = []
  for (const harness of harnesses) {
    const setup = harness.setup
    if (setup === undefined) {
      continue
    }
    const paths = agentPathsOf(setup, configDirectory, env, home)
    agents.push({ id: harness.id, setup, paths })
  }
  return agents
}

// A development build writes only where both directories were pointed elsewhere,
// so developing the app can never rewrite the real settings of an installed agent.
export function canWriteFor(setup: HarnessSetup, isStable: boolean, env: NodeJS.ProcessEnv): boolean {
  if (isStable) {
    return true
  }
  const hasAppOverride = isSet(env[DEV_CONFIG_DIRECTORY_VARIABLE])
  const hasAgentOverride = isSet(env[setup.configDirectoryVariable])
  return hasAppOverride && hasAgentOverride
}

export function readState(agent: Agent, managing: boolean): AgentSetupState {
  const installed = existsSync(agent.paths.agentDirectory)
  const settings = readSettings(agent.paths.settingsPath)
  const wiring = hookWiringOf(settings.text, agent.setup, agent.paths.hooksDirectory)
  const record = readAgentRecord(agent.paths.configDirectory, agent.id)
  return agentSetupStateOf({ installed, managing, wiring, declined: record.declined })
}

export function setUp(agent: Agent): string | null {
  const settings = readSettings(agent.paths.settingsPath)
  if (settings.problem !== null) {
    return settings.problem
  }
  const added = addEntries(
    settings.text,
    agent.setup.entries,
    agent.paths.hooksDirectory,
    agent.paths.settingsPath
  )
  if (added.problem !== null) {
    return added.problem
  }
  const writeProblem = writeChanged(agent, added)
  if (writeProblem !== null) {
    return writeProblem
  }
  return saveSetUpRecord(agent)
}

export function removeSetup(agent: Agent): string | null {
  const settings = readSettings(agent.paths.settingsPath)
  if (settings.problem !== null) {
    return settings.problem
  }
  if (settings.text !== null) {
    const removed = removeManagedEntries(settings.text, agent.paths.hooksDirectory, agent.paths.settingsPath)
    if (removed.problem !== null) {
      return removed.problem
    }
    const writeProblem = writeChanged(agent, removed)
    if (writeProblem !== null) {
      return writeProblem
    }
  }
  return saveRecord(agent, { declined: true, written: [] })
}

export function switchToManaged(agent: Agent): string | null {
  const settings = readSettings(agent.paths.settingsPath)
  if (settings.problem !== null) {
    return settings.problem
  }
  let text = settings.text
  if (text !== null) {
    const cleared = removeHandEntries(
      text,
      agent.setup,
      agent.paths.hooksDirectory,
      agent.paths.settingsPath
    )
    if (cleared.problem !== null) {
      return cleared.problem
    }
    if (cleared.text !== null) {
      text = cleared.text
    }
  }
  const added = addEntries(text, agent.setup.entries, agent.paths.hooksDirectory, agent.paths.settingsPath)
  if (added.problem !== null) {
    return added.problem
  }
  if (added.text !== null) {
    text = added.text
  }
  if (text !== null && text !== settings.text) {
    const writeProblem = writeSettings(agent.paths.settingsPath, text)
    if (writeProblem !== null) {
      return writeProblem
    }
  }
  return saveSetUpRecord(agent)
}

// Adds the entries the record has never written, which a newer version introduced.
// One it wrote before and no longer finds was deleted by the user, and stays deleted.
export function keepCurrent(agent: Agent): string | null {
  const record = readAgentRecord(agent.paths.configDirectory, agent.id)
  const missing = agent.setup.entries.filter((entry) => {
    const key = entryKey(entry)
    return !record.written.includes(key)
  })
  if (missing.length === 0) {
    return null
  }
  const settings = readSettings(agent.paths.settingsPath)
  if (settings.problem !== null) {
    return settings.problem
  }
  const added = addEntries(settings.text, missing, agent.paths.hooksDirectory, agent.paths.settingsPath)
  if (added.problem !== null) {
    return added.problem
  }
  const writeProblem = writeChanged(agent, added)
  if (writeProblem !== null) {
    return writeProblem
  }
  const missingKeys = missing.map(entryKey)
  const written = [...record.written, ...missingKeys]
  return saveRecord(agent, { declined: record.declined, written })
}

export function decline(agent: Agent): string | null {
  const record = readAgentRecord(agent.paths.configDirectory, agent.id)
  return saveRecord(agent, { declined: true, written: record.written })
}
