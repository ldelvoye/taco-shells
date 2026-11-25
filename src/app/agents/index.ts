import { homedir } from 'node:os'
import { HARNESSES } from '@agents/registry'
import type { AgentBanner } from '@protocol/agentSetup'
import type { SessionId } from '@protocol/ipc'
import { foregroundCommands } from '@runtime/foreground'
import type { PtySessions } from '@runtime/sessions'
import { isStable } from '../channel'
import type { ConfigStore } from '../config'
import type { Service } from '../service'
import { agentsOf, canWriteFor, type Agent } from './flow'
import { copyHookScripts, hooksDirectory } from './scripts'
import { AgentSetups, type AgentSetupsEvents } from './setups'

export interface AgentsHost {
  hookScriptsSource: string
  showFolder(path: string): void
}

export interface AgentsService extends Service {
  banners(): AgentBanner[]
  setUp(agentId: string): void
  decline(agentId: string): void
  dismiss(agentId: string): void
  refresh(): void
}

/** The agents' hook scripts and menu entries, kept current while the app runs. */
export function startAgents(
  configDirectory: string,
  store: ConfigStore,
  sessions: PtySessions,
  host: AgentsHost,
  events: AgentSetupsEvents
): AgentsService {
  const destination = hooksDirectory(configDirectory)
  const copyProblems = copyHookScripts(host.hookScriptsSource, destination)
  for (const problem of copyProblems) {
    console.error(problem)
  }

  const home = homedir()
  const agents = agentsOf(HARNESSES, configDirectory, process.env, home)

  const setups = new AgentSetups({
    agents,
    isManaging: () => store.current().settings.manageAgentHooks,
    canWrite: (agent: Agent) => canWriteFor(agent.setup, isStable, process.env),
    foregroundCommands: async (id: SessionId) => {
      const pid = sessions.pidOf(id)
      if (pid === null) {
        return []
      }
      return foregroundCommands(pid)
    },
    showScripts: (agent: Agent) => {
      host.showFolder(agent.paths.hooksDirectory)
    },
    events
  })

  sessions.onSubmit((id) => {
    setups.noteSubmit(id)
  })

  setups.start()
  store.onChange(() => {
    setups.refresh()
  })

  return {
    banners: () => setups.banners(),
    setUp: (agentId: string) => {
      setups.act(agentId, 'setUp')
    },
    decline: (agentId: string) => {
      setups.decline(agentId)
    },
    dismiss: (agentId: string) => {
      setups.dismiss(agentId)
    },
    refresh: () => {
      setups.refresh()
    },
    stop: () => {
      setups.stop()
    }
  }
}
