import { homedir } from 'node:os'
import { app, type BrowserWindow, ipcMain, shell } from 'electron'
import {
  agentBannerOf,
  agentMenuOf,
  agentNoticeAfter,
  type AgentBanner,
  type AgentMenu,
  type AgentMenuAction,
  type AgentNotice,
  type AgentSetupState
} from '@shared/agentSetup'
import { HARNESSES } from '@shared/harness/registry'
import { CHANNEL } from '@shared/ipc'
import { isStable } from '../channel'
import type { ConfigStore } from '../config'
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
  menu: ApplicationMenu
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
    return agentBannerOf(agent.id, agent.setup.productName, state, mayWrite, notice)
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

  refresh()
  app.on('did-become-active', onActive)
  store.onChange(() => {
    refresh()
  })

  return {
    stop: () => {
      app.off('did-become-active', onActive)
      ipcMain.removeAllListeners(CHANNEL.agentsGet)
      ipcMain.removeAllListeners(CHANNEL.agentsSetUp)
      ipcMain.removeAllListeners(CHANNEL.agentsDecline)
      ipcMain.removeAllListeners(CHANNEL.agentsDismiss)
    }
  }
}
