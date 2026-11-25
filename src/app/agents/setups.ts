import {
  agentBannerOf,
  agentMenuOf,
  agentNoticeAfter,
  runsAgent,
  type AgentMenu,
  type AgentMenuAction,
  type AgentNotice,
  type AgentSetupState
} from './agentSetup'
import type { AgentBanner } from '@protocol/agentSetup'
import type { SessionId } from '@protocol/ipc'
import {
  decline,
  keepCurrent,
  readState,
  removeSetup,
  setUp,
  switchToManaged,
  type Agent
} from './flow'

// Long enough for the shell to have started the command that was just submitted.
export const SUBMIT_CHECK_DELAY_MS = 1000

export interface AgentMenuEntry {
  name: string
  menu: AgentMenu
  onAction: () => void
  onShowScripts: () => void
}

export interface AgentSetupsEvents {
  onMenuChanged(entries: AgentMenuEntry[]): void
  onBannersChanged(banners: AgentBanner[]): void
}

export interface AgentSetupsOptions {
  agents: readonly Agent[]
  isManaging: () => boolean
  canWrite: (agent: Agent) => boolean
  foregroundCommands: (id: SessionId) => Promise<readonly string[]>
  showScripts: (agent: Agent) => void
  events: AgentSetupsEvents
}

interface AgentReading {
  agent: Agent
  state: AgentSetupState
  mayWrite: boolean
}

function reportProblem(problem: string | null): void {
  if (problem !== null) {
    console.error(problem)
  }
}

function perform(action: AgentMenuAction, agent: Agent): string | null {
  switch (action) {
    case 'setUp':
      return setUp(agent)
    case 'remove':
      return removeSetup(agent)
    case 'switch':
      return switchToManaged(agent)
  }
}

function menuFrom(reading: AgentReading): AgentMenu {
  const productName = reading.agent.setup.productName
  return agentMenuOf(reading.state, productName, reading.mayWrite)
}

export class AgentSetups {
  private readonly options: AgentSetupsOptions
  // How each agent's last action went, kept until dismissed and shown in place of the offer.
  private readonly notices = new Map<string, AgentNotice>()
  private sentBanners = ''
  // Agents seen running in a terminal since launch. Not saved, so an unanswered offer waits for the next run.
  private readonly seen = new Set<string>()
  // Agents whose offer only waits on being seen. Kept by refresh, so a submitted line costs nothing while it is empty.
  private awaiting: Agent[] = []
  private readonly pendingChecks = new Map<SessionId, ReturnType<typeof setTimeout>>()

  constructor(options: AgentSetupsOptions) {
    this.options = options
  }

  start(): void {
    for (const agent of this.options.agents) {
      if (this.options.canWrite(agent) && this.stateOf(agent) === 'setUp') {
        const problem = keepCurrent(agent)
        reportProblem(problem)
      }
    }
    this.refresh()
  }

  stop(): void {
    for (const timer of this.pendingChecks.values()) {
      clearTimeout(timer)
    }
    this.pendingChecks.clear()
  }

  refresh(): void {
    const readings = this.readAll()
    const awaitingReadings = readings.filter((reading) => {
      return !this.seen.has(reading.agent.id) && reading.mayWrite && reading.state === 'notSetUp'
    })
    this.awaiting = awaitingReadings.map((reading) => reading.agent)
    const entries = readings.map((reading) => this.entryOf(reading))
    this.options.events.onMenuChanged(entries)
    const banners = this.bannersFrom(readings)
    const serialized = JSON.stringify(banners)
    if (serialized !== this.sentBanners) {
      this.sentBanners = serialized
      this.options.events.onBannersChanged(banners)
    }
  }

  banners(): AgentBanner[] {
    const readings = this.readAll()
    const banners = this.bannersFrom(readings)
    // The synchronous read counts as sent, so the next refresh skips an identical push.
    this.sentBanners = JSON.stringify(banners)
    return banners
  }

  // The file may have changed since the menu was built, so a click acts only
  // when the menu would still offer the action that was clicked.
  act(agentId: string, clicked: AgentMenuAction): void {
    const agent = this.agentWith(agentId)
    if (agent === undefined) {
      return
    }
    const reading = this.readingOf(agent)
    const fresh = menuFrom(reading)
    if (fresh.enabled && fresh.action === clicked) {
      const problem = perform(clicked, agent)
      this.noteOutcome(agent, clicked, problem)
    }
    this.refresh()
  }

  decline(agentId: string): void {
    const agent = this.agentWith(agentId)
    if (agent === undefined) {
      return
    }
    const problem = decline(agent)
    if (problem !== null) {
      console.error(problem)
      this.notices.set(agent.id, { kind: 'problem', text: problem })
    }
    this.refresh()
  }

  dismiss(agentId: string): void {
    const agent = this.agentWith(agentId)
    if (agent === undefined) {
      return
    }
    this.notices.delete(agent.id)
    this.refresh()
  }

  // An offer waits until its agent is seen running, so a launch never opens with a row of strips.
  noteSubmit(id: SessionId): void {
    if (this.awaiting.length === 0) {
      return
    }
    const earlier = this.pendingChecks.get(id)
    if (earlier !== undefined) {
      clearTimeout(earlier)
    }
    const timer = setTimeout(() => {
      this.pendingChecks.delete(id)
      void this.noticeAgentsIn(id)
    }, SUBMIT_CHECK_DELAY_MS)
    this.pendingChecks.set(id, timer)
  }

  private agentWith(agentId: string): Agent | undefined {
    return this.options.agents.find((candidate) => candidate.id === agentId)
  }

  private stateOf(agent: Agent): AgentSetupState {
    const managing = this.options.isManaging()
    return readState(agent, managing)
  }

  private readingOf(agent: Agent): AgentReading {
    const state = this.stateOf(agent)
    const mayWrite = this.options.canWrite(agent)
    return { agent, state, mayWrite }
  }

  private readAll(): AgentReading[] {
    return this.options.agents.map((agent) => this.readingOf(agent))
  }

  private bannersFrom(readings: readonly AgentReading[]): AgentBanner[] {
    const banners: AgentBanner[] = []
    for (const reading of readings) {
      const banner = this.bannerFrom(reading)
      if (banner !== null) {
        banners.push(banner)
      }
    }
    return banners
  }

  private bannerFrom(reading: AgentReading): AgentBanner | null {
    const agent = reading.agent
    let notice: AgentNotice | null = null
    const found = this.notices.get(agent.id)
    if (found !== undefined) {
      notice = found
    }
    const wasSeen = this.seen.has(agent.id)
    return agentBannerOf(agent.id, agent.setup.productName, reading.state, reading.mayWrite, notice, wasSeen)
  }

  private entryOf(reading: AgentReading): AgentMenuEntry {
    const agent = reading.agent
    const published = menuFrom(reading)
    return {
      name: agent.setup.productName,
      menu: published,
      onAction: () => {
        this.act(agent.id, published.action)
      },
      onShowScripts: () => {
        this.options.showScripts(agent)
      }
    }
  }

  private noteOutcome(agent: Agent, clicked: AgentMenuAction, problem: string | null): void {
    reportProblem(problem)
    const notice = agentNoticeAfter(clicked, problem)
    if (notice === null) {
      this.notices.delete(agent.id)
    } else {
      this.notices.set(agent.id, notice)
    }
  }

  private async noticeAgentsIn(id: SessionId): Promise<void> {
    const commandLines = await this.options.foregroundCommands(id)
    let added = false
    for (const agent of this.awaiting) {
      if (runsAgent(commandLines, agent.setup.commands)) {
        this.seen.add(agent.id)
        added = true
      }
    }
    if (added) {
      this.refresh()
    }
  }
}
