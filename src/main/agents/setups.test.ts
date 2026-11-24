import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentBanner } from '@shared/agentSetup'
import { claude } from '@shared/harness/claude'
import type { HarnessSetup } from '@shared/harness/types'
import type { SessionId } from '@shared/ipc'
import { agentPathsOf, type Agent } from './flow'
import { AgentSetups, SUBMIT_CHECK_DELAY_MS, type AgentSetupsOptions } from './setups'

const MISSING_HOME = '/nonexistent-home'
const RECORD_FILE = 'agents.json'
const TERMINAL: SessionId = 'terminal-1'

const setup = claude.setup as HarnessSetup

let appDirectory = ''
let agentDirectory = ''

interface Sent {
  menuChanges: number
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
  const sent: Sent = { menuChanges: 0, lastBanners: [] }
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
  })
})
