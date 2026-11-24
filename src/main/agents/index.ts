import { homedir } from 'node:os'
import { app, type BrowserWindow, ipcMain, shell } from 'electron'
import { HARNESSES } from '@shared/harness/registry'
import { CHANNEL, type SessionId } from '@shared/ipc'
import { isStable } from '../channel'
import type { ConfigStore } from '../config'
import { foregroundCommands } from '../pty/foreground'
import type { PtySessions } from '../pty/sessions'
import type { ApplicationMenu } from '../window/applicationMenu'
import { sendToWindow } from '../window/send'
import type { Service } from '../service'
import { agentsOf, canWriteFor, type Agent } from './flow'
import { copyHookScripts, hookScriptsSource, hooksDirectory } from './scripts'
import { AgentSetups } from './setups'

function onAgent(channel: string, handle: (agentId: string) => void): void {
  ipcMain.on(channel, (_event, id: unknown) => {
    if (typeof id === 'string') {
      handle(id)
    }
  })
}

/** The agents' hook scripts and menu entries, kept current while the app runs. */
export function registerAgents(
  window: BrowserWindow,
  configDirectory: string,
  store: ConfigStore,
  menu: ApplicationMenu,
  sessions: PtySessions
): Service {
  const source = hookScriptsSource(app.isPackaged, process.resourcesPath, app.getAppPath())
  const destination = hooksDirectory(configDirectory)
  const copyProblems = copyHookScripts(source, destination)
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
      void shell.openPath(agent.paths.hooksDirectory)
    },
    events: {
      onMenuChanged: (entries) => {
        menu.setAgents(entries)
      },
      onBannersChanged: (banners) => {
        sendToWindow(window, CHANNEL.agentsChanged, banners)
      }
    }
  })

  // A sendSync left unanswered hangs the renderer at startup, so this answers
  // even when no harness declares a setup.
  ipcMain.on(CHANNEL.agentsGet, (event) => {
    event.returnValue = setups.banners()
  })

  onAgent(CHANNEL.agentsSetUp, (agentId) => {
    setups.act(agentId, 'setUp')
  })

  onAgent(CHANNEL.agentsDecline, (agentId) => {
    setups.decline(agentId)
  })

  onAgent(CHANNEL.agentsDismiss, (agentId) => {
    setups.dismiss(agentId)
  })

  const onActive = (): void => {
    setups.refresh()
  }

  sessions.onSubmit((id) => {
    setups.noteSubmit(id)
  })

  setups.start()
  app.on('did-become-active', onActive)
  store.onChange(() => {
    setups.refresh()
  })

  return {
    stop: () => {
      setups.stop()
      app.off('did-become-active', onActive)
      ipcMain.removeAllListeners(CHANNEL.agentsGet)
      ipcMain.removeAllListeners(CHANNEL.agentsSetUp)
      ipcMain.removeAllListeners(CHANNEL.agentsDecline)
      ipcMain.removeAllListeners(CHANNEL.agentsDismiss)
    }
  }
}
