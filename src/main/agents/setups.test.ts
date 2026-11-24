import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentBanner } from '@shared/agentSetup'
import { claude } from '@shared/harness/claude'
import type { HarnessSetup } from '@shared/harness/types'
import type { SessionId } from '@shared/ipc'
import { agentPathsOf, setUp, type Agent } from './flow'
import { readAgentRecord } from './record'
import { AgentSetups, SUBMIT_CHECK_DELAY_MS, type AgentSetupsOptions } from './setups'

const MISSING_HOME = '/nonexistent-home'
const RECORD_FILE = 'agents.json'
const TERMINAL: SessionId = 'terminal-1'

const setup = claude.setup as HarnessSetup

let appDirectory = ''
let agentDirectory = ''

interface Sent {
  menuChanges: number
  bannerPushes: number
  lastBanners: AgentBanner[]
}

interface Harnessed {
  setups: AgentSetups
  options: AgentSetupsOptions
  sent: Sent
}

function agentWith(id: string): Agent {
  const env = { CLAUDE_CONFIG_DIR: agentDirectory }
  const paths = agentPathsOf(setup, appDirectory, env, MISSING_HOME)
  return { id, setup, paths }
}

function harness(agents: Agent[], commandLines: readonly string[]): Harnessed {
  const sent: Sent = { menuChanges: 0, bannerPushes: 0, lastBanners: [] }
  const options: AgentSetupsOptions = {
    agents,
    isManaging: vi.fn(() => true),
    canWrite: () => true,
    foregroundCommands: vi.fn(async () => commandLines),
    showScripts: () => undefined,
    events: {
      onMenuChanged: () => {
        sent.menuChanges += 1
      },
      onBannersChanged: (banners) => {
        sent.bannerPushes += 1
        sent.lastBanners = banners
      }
    }
  }
  const setups = new AgentSetups(options)
  return { setups, options, sent }
}

beforeEach(() => {
  appDirectory = mkdtempSync(join(tmpdir(), 'taco-shells-setups-'))
  agentDirectory = mkdtempSync(join(tmpdir(), 'taco-shells-setups-'))
})

afterEach(() => {
  vi.useRealTimers()
  rmSync(appDirectory, { recursive: true, force: true })
  rmSync(agentDirectory, { recursive: true, force: true })
})

describe('agent setups', () => {
  it('acts on a click only when the menu would still offer that action', () => {
    const agent = agentWith(claude.id)
    const { setups, sent } = harness([agent], [])
    setups.refresh()
    setups.act(claude.id, 'setUp')

    const recordPath = join(appDirectory, RECORD_FILE)
    const recordText = readFileSync(recordPath, 'utf8')
    const compactRecord = JSON.stringify(JSON.parse(recordText))
    writeFileSync(recordPath, compactRecord)
    const settingsText = readFileSync(agent.paths.settingsPath, 'utf8')
    const compactSettings = JSON.stringify(JSON.parse(settingsText))
    writeFileSync(agent.paths.settingsPath, compactSettings)
    const changesBefore = sent.menuChanges

    setups.act(claude.id, 'setUp')

    expect(readFileSync(recordPath, 'utf8')).toBe(compactRecord)
    expect(readFileSync(agent.paths.settingsPath, 'utf8')).toBe(compactSettings)
    expect(sent.menuChanges).toBe(changesBefore + 1)
  })

  it('holds an offer back until its agent is seen running', async () => {
    vi.useFakeTimers()
    const harnessed = harness([agentWith(claude.id)], ['claude --resume'])
    harnessed.setups.start()
    expect(harnessed.sent.lastBanners).toEqual([])

    harnessed.setups.noteSubmit(TERMINAL)
    await vi.advanceTimersByTimeAsync(SUBMIT_CHECK_DELAY_MS)

    const kinds = harnessed.sent.lastBanners.map((banner) => banner.kind)
    expect(kinds).toEqual(['offer'])
  })

  it('starts no check for a submitted line while nobody is awaited', async () => {
    vi.useFakeTimers()
    const harnessed = harness([agentWith(claude.id)], ['claude'])
    harnessed.setups.start()
    harnessed.setups.decline(claude.id)

    harnessed.setups.noteSubmit(TERMINAL)
    await vi.advanceTimersByTimeAsync(SUBMIT_CHECK_DELAY_MS)

    expect(harnessed.options.foregroundCommands).not.toHaveBeenCalled()
  })

  it('reads each agent once per refresh', () => {
    const agents = [agentWith('first'), agentWith('second')]
    const harnessed = harness(agents, [])

    harnessed.setups.refresh()

    expect(harnessed.options.isManaging).toHaveBeenCalledTimes(agents.length)
  })

  it('leaves a problem notice when a decline cannot be written', () => {
    mkdirSync(join(appDirectory, RECORD_FILE))
    const harnessed = harness([agentWith(claude.id)], [])
    vi.spyOn(console, 'error').mockImplementation(() => undefined)

    harnessed.setups.decline(claude.id)

    const kinds = harnessed.sent.lastBanners.map((banner) => banner.kind)
    expect(kinds).toEqual(['problem'])

    harnessed.setups.dismiss(claude.id)

    expect(harnessed.sent.lastBanners).toEqual([])
  })

  it('keeps the outcome of an action as a notice until dismissed', () => {
    const harnessed = harness([agentWith(claude.id)], [])
    harnessed.setups.refresh()

    harnessed.setups.act(claude.id, 'setUp')

    const kinds = harnessed.sent.lastBanners.map((banner) => banner.kind)
    expect(kinds).toEqual(['done'])

    harnessed.setups.dismiss(claude.id)

    expect(harnessed.sent.lastBanners).toEqual([])
  })

  it('pushes banners only when they changed since the last ones sent', () => {
    const harnessed = harness([agentWith(claude.id)], [])

    harnessed.setups.banners()
    harnessed.setups.refresh()
    harnessed.setups.refresh()

    expect(harnessed.sent.bannerPushes).toBe(0)
  })

  it('replaces a pending check when the same terminal submits again', async () => {
    vi.useFakeTimers()
    const harnessed = harness([agentWith(claude.id)], [])
    harnessed.setups.start()

    harnessed.setups.noteSubmit(TERMINAL)
    await vi.advanceTimersByTimeAsync(SUBMIT_CHECK_DELAY_MS / 2)
    harnessed.setups.noteSubmit(TERMINAL)
    await vi.advanceTimersByTimeAsync(SUBMIT_CHECK_DELAY_MS)

    expect(harnessed.options.foregroundCommands).toHaveBeenCalledTimes(1)
  })

  it('cancels pending checks when stopped', async () => {
    vi.useFakeTimers()
    const harnessed = harness([agentWith(claude.id)], [])
    harnessed.setups.start()

    harnessed.setups.noteSubmit(TERMINAL)
    harnessed.setups.stop()
    await vi.advanceTimersByTimeAsync(SUBMIT_CHECK_DELAY_MS)

    expect(harnessed.options.foregroundCommands).not.toHaveBeenCalled()
  })

  it('adds the entries a set-up agent is missing when it starts', () => {
    const agent = agentWith(claude.id)
    setUp(agent)
    const settingsText = readFileSync(agent.paths.settingsPath, 'utf8')
    const settings = JSON.parse(settingsText) as { hooks: Record<string, unknown> }
    delete settings.hooks.StopFailure
    const staleSettings = JSON.stringify(settings)
    writeFileSync(agent.paths.settingsPath, staleSettings)
    const record = readAgentRecord(appDirectory, claude.id)
    const written = record.written.filter((key) => !key.startsWith('StopFailure:'))
    const staleRecord = JSON.stringify({ [claude.id]: { declined: false, written } })
    writeFileSync(join(appDirectory, RECORD_FILE), staleRecord)
    const harnessed = harness([agent], [])

    harnessed.setups.start()

    const kept = readAgentRecord(appDirectory, claude.id)
    expect(kept.written).toHaveLength(setup.entries.length)
    const after = readFileSync(agent.paths.settingsPath, 'utf8')
    expect(after).toContain('StopFailure')
  })
})
