import { homedir } from 'node:os'
import { app, type BrowserWindow, ipcMain, shell } from 'electron'
import {
  agentBannerOf,
  agentMenuOf,
  agentNoticeAfter,
  runsAgent,
  type AgentBanner,
  type AgentMenu,
  type AgentMenuAction,
  type AgentNotice,
  type AgentSetupState
} from '@shared/agentSetup'
import { HARNESSES } from '@shared/harness/registry'
import { CHANNEL, type SessionId } from '@shared/ipc'
import { isStable } from '../channel'
import type { ConfigStore } from '../config'
import { foregroundCommands } from '../pty/foreground'
import type { PtySessions } from '../pty/sessions'
import type { AgentMenuEntry } from '../window/menu'
import type { ApplicationMenu } from '../window/applicationMenu'
import {
  agentsOf,
  canWriteFor,
  decline,
  keepCurrent,
  readState,
  removeSetup,
  setUp,
  switchToManaged,
  type Agent
} from './flow'
import { copyHookScripts, hookScriptsSource, hooksDirectory } from './scripts'

// Long enough for the shell to have started the command that was just submitted.
const SUBMIT_CHECK_DELAY_MS = 1000

export interface RegisteredAgents {
  stop: () => void
}

function reportProblem(problem: string | null): void {
  if (problem !== null) {
    console.error(problem)
  }
}

function sendToWindow(window: BrowserWindow, channel: string, payload: AgentBanner[]): void {
  if (window.isDestroyed()) {
    return
  }
  window.webContents.send(channel, payload)
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

/** The agents' hook scripts and menu entries, kept current while the app runs. */
export function registerAgents(
  window: BrowserWindow,
  configDirectory: string,
  store: ConfigStore,
  menu: ApplicationMenu,
  sessions: PtySessions
): RegisteredAgents {
  const source = hookScriptsSource(app.isPackaged, process.resourcesPath, app.getAppPath())
  const destination = hooksDirectory(configDirectory)
  const copyProblems = copyHookScripts(source, destination)
  for (const problem of copyProblems) {
    console.error(problem)
  }

  const home = homedir()
  const agents = agentsOf(HARNESSES, configDirectory, process.env, home)
  // How each agent's last action went, kept until dismissed and shown in place of the offer.
  const notices = new Map<string, AgentNotice>()
  let sentBanners = ''
  // Agents seen running in a terminal since launch. Not saved, so an unanswered offer waits for the next run.
  const seen = new Set<string>()
  // Agents whose offer only waits on being seen. Kept by refresh, so a submitted line costs nothing while it is empty.
  let awaiting: Agent[] = []
  const pendingChecks = new Map<SessionId, ReturnType<typeof setTimeout>>()

  function canWrite(agent: Agent): boolean {
    return canWriteFor(agent.setup, isStable, process.env)
  }

  function stateOf(agent: Agent): AgentSetupState {
    const managing = store.current().settings.manageAgentHooks
    return readState(agent, managing)
  }

  function menuOf(agent: Agent): AgentMenu {
    const state = stateOf(agent)
    const mayWrite = canWrite(agent)
    return agentMenuOf(state, agent.setup.productName, mayWrite)
  }

  function bannerOf(agent: Agent): AgentBanner | null {
    const state = stateOf(agent)
    const mayWrite = canWrite(agent)
    let notice: AgentNotice | null = null
    const found = notices.get(agent.id)
    if (found !== undefined) {
      notice = found
    }
    const wasSeen = seen.has(agent.id)
    return agentBannerOf(agent.id, agent.setup.productName, state, mayWrite, notice, wasSeen)
  }

  function bannersOf(): AgentBanner[] {
    const banners: AgentBanner[] = []
    for (const agent of agents) {
      const banner = bannerOf(agent)
      if (banner !== null) {
        banners.push(banner)
      }
    }
    return banners
  }

  function noteOutcome(agent: Agent, clicked: AgentMenuAction, problem: string | null): void {
    if (problem !== null) {
      console.error(problem)
    }
    const notice = agentNoticeAfter(clicked, problem)
    if (notice === null) {
      notices.delete(agent.id)
    } else {
      notices.set(agent.id, notice)
    }
  }

  // The file may have changed since the menu was built, so a click acts only
  // when the menu would still offer the action that was clicked.
  function act(agent: Agent, clicked: AgentMenuAction): void {
    const fresh = menuOf(agent)
    if (fresh.enabled && fresh.action === clicked) {
      const problem = perform(clicked, agent)
      noteOutcome(agent, clicked, problem)
    }
    refresh()
  }

  function onAgent(channel: string, handle: (agent: Agent) => void): void {
    ipcMain.on(channel, (_event, id: unknown) => {
      const agent = agents.find((candidate) => candidate.id === id)
      if (agent !== undefined) {
        handle(agent)
      }
    })
  }

  function entryOf(agent: Agent): AgentMenuEntry {
    const published = menuOf(agent)
    return {
      name: agent.setup.productName,
      menu: published,
      onAction: () => {
        act(agent, published.action)
      },
      onShowScripts: () => {
        void shell.openPath(agent.paths.hooksDirectory)
      }
    }
  }

  function refresh(): void {
    awaiting = agents.filter((agent) => {
      return !seen.has(agent.id) && canWrite(agent) && stateOf(agent) === 'notSetUp'
    })
    const entries = agents.map(entryOf)
    menu.setAgents(entries)
    const banners = bannersOf()
    const serialized = JSON.stringify(banners)
    if (serialized !== sentBanners) {
      sentBanners = serialized
      sendToWindow(window, CHANNEL.agentsChanged, banners)
    }
  }

  // A sendSync left unanswered hangs the renderer at startup, so this answers
  // even when no harness declares a setup.
  ipcMain.on(CHANNEL.agentsGet, (event) => {
    const banners = bannersOf()
    sentBanners = JSON.stringify(banners)
    event.returnValue = banners
  })

  onAgent(CHANNEL.agentsSetUp, (agent) => {
    act(agent, 'setUp')
  })

  onAgent(CHANNEL.agentsDecline, (agent) => {
    const problem = decline(agent)
    if (problem !== null) {
      console.error(problem)
      notices.set(agent.id, { kind: 'problem', text: problem })
    }
    refresh()
  })

  onAgent(CHANNEL.agentsDismiss, (agent) => {
    notices.delete(agent.id)
    refresh()
  })

  for (const agent of agents) {
    if (canWrite(agent) && stateOf(agent) === 'setUp') {
      const problem = keepCurrent(agent)
      reportProblem(problem)
    }
  }

  const onActive = (): void => {
    refresh()
  }

  async function noticeAgentsIn(id: SessionId): Promise<void> {
    const pid = sessions.pidOf(id)
    if (pid === null) {
      return
    }
    const commandLines = await foregroundCommands(pid)
    let added = false
    for (const agent of awaiting) {
      if (runsAgent(commandLines, agent.setup.commands)) {
        seen.add(agent.id)
        added = true
      }
    }
    if (added) {
      refresh()
    }
  }

  // An offer waits until its agent is seen running, so a launch never opens with a row of strips.
  sessions.onSubmit((id) => {
    if (awaiting.length === 0) {
      return
    }
    const earlier = pendingChecks.get(id)
    if (earlier !== undefined) {
      clearTimeout(earlier)
    }
    const timer = setTimeout(() => {
      pendingChecks.delete(id)
      void noticeAgentsIn(id)
    }, SUBMIT_CHECK_DELAY_MS)
    pendingChecks.set(id, timer)
  })

  refresh()
  app.on('did-become-active', onActive)
  store.onChange(() => {
    refresh()
  })

  return {
    stop: () => {
      for (const timer of pendingChecks.values()) {
        clearTimeout(timer)
      }
      pendingChecks.clear()
      app.off('did-become-active', onActive)
      ipcMain.removeAllListeners(CHANNEL.agentsGet)
      ipcMain.removeAllListeners(CHANNEL.agentsSetUp)
      ipcMain.removeAllListeners(CHANNEL.agentsDecline)
      ipcMain.removeAllListeners(CHANNEL.agentsDismiss)
    }
  }
}
