import { homedir } from 'node:os'
import { app, shell } from 'electron'
import {
  agentMenuOf,
  type AgentMenu,
  type AgentMenuAction,
  type AgentSetupState
} from '@shared/agentSetup'
import { HARNESSES } from '@shared/harness/registry'
import { isStable } from '../channel'
import type { ConfigStore } from '../config'
import type { AgentMenuEntry } from '../window/menu'
import type { ApplicationMenu } from '../window/applicationMenu'
import {
  agentsOf,
  canWriteFor,
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

  // The file may have changed since the menu was built, so a click acts only
  // when the menu would still offer the action that was clicked.
  function act(agent: Agent, clicked: AgentMenuAction): void {
    const fresh = menuOf(agent)
    if (fresh.enabled && fresh.action === clicked) {
      const problem = perform(clicked, agent)
      reportProblem(problem)
    }
    refresh()
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
  }

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
    }
  }
}
