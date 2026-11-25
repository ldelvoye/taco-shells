import { app, net, shell } from 'electron'
import type { ConfigFile } from '@protocol/config'
import { PtySessions } from '@runtime/sessions'
import { paletteFor } from '@settings/theme'
import { AgentSessions } from '@workspace/agentSessions'
import { SnapshotStore } from '@workspace/snapshot/store'
import { startAgents, type AgentsHost } from '../agents'
import { hookScriptsSource } from '../agents/scripts'
import { applicationName } from '../channel'
import { ConfigStore } from '../config'
import { configDirectory, configFilePath } from '../config/files'
import { startFocus } from '../focus'
import type { Service } from '../service'
import { startUpdates, type UpdateHost } from '../update'
import { registerIpc } from './ipc'
import { bridgeAgents } from './ipc/agents'
import { bridgeFocus } from './ipc/focus'
import { registerHostApi } from './ipc/hostApi'
import { bridgeUpdates } from './ipc/updates'
import { bridgeWorkspace } from './ipc/workspace'
import { nativeSystemTheme } from './systemTheme'
import { ApplicationMenu } from './window/applicationMenu'
import { linkTo, type WindowLink } from './window/link'
import { createMainWindow, isBackgroundWindow } from './window/window'

app.setName(applicationName)

const sessions = new PtySessions()
const agentSessions = new AgentSessions()
const services: Service[] = []
let snapshotStore: SnapshotStore | undefined
let flushed = false

app.whenReady().then(() => {
  // Launching a GUI app activates it and takes the user's focus, so a suite run
  // has to leave the dock alone.
  if (isBackgroundWindow()) {
    app.dock?.hide()
  }

  const applicationMenu = new ApplicationMenu()
  applicationMenu.show()

  const directory = configDirectory()
  const configStore = new ConfigStore(directory, nativeSystemTheme())
  const initialConfig = configStore.current()
  const palette = paletteFor(initialConfig.appearance)
  const link = linkTo(createMainWindow(palette))
  const snapshots = new SnapshotStore(directory, agentSessions, (id) => sessions.directoryOf(id))
  snapshotStore = snapshots
  const restoreOnLaunch = initialConfig.settings.restoreOnLaunch
  const configPathOf = (file: ConfigFile): string => configFilePath(directory, file)
  const workspaceBridge = bridgeWorkspace(
    link,
    snapshots,
    agentSessions,
    sessions,
    configPathOf,
    restoreOnLaunch
  )
  const workspace = workspaceBridge.workspace
  registerIpc(sessions, link, configStore)
  registerHostApi(link)

  const updateHost: UpdateHost = {
    appVersion: app.getVersion(),
    executablePath: app.getPath('exe'),
    resourcesPath: process.resourcesPath,
    fetch: (input, init) => net.fetch(input, init),
    quit: () => {
      app.quit()
    }
  }
  const updates = bridgeUpdates(link, applicationMenu, (events) => {
    return startUpdates(directory, updateHost, events)
  })

  const agentsHost: AgentsHost = {
    hookScriptsSource: hookScriptsSource(app.isPackaged, process.resourcesPath, app.getAppPath()),
    showFolder: (path) => {
      void shell.openPath(path)
    }
  }
  const agents = bridgeAgents(link, applicationMenu, (events) => {
    return startAgents(directory, configStore, sessions, agentsHost, events)
  })

  const focus = bridgeFocus(link, workspace, (events) => {
    return startFocus(sessions, directory, events, () => {
      updates.checkNow()
    })
  })
  services.push(configStore, workspaceBridge, focus, updates, agents, snapshots)

  whenTheWindowCloses(link, snapshots, agentSessions)

  configStore.onChange((config) => {
    const nextPalette = paletteFor(config.appearance)
    link.setBackgroundColor(nextPalette.chrome.terminalBackground)
  })

  // Last, so everything that listens to a shell is in place before the first one starts.
  workspace.start()
})

// The one place that decides what the window closing means: it ends the app.
function whenTheWindowCloses(
  link: WindowLink,
  snapshots: SnapshotStore,
  sessionsOfAgents: AgentSessions
): void {
  // Closing the window is the start of a quit, and the snapshot has to stop
  // changing from that moment, before the ptys go down with the process.
  link.onClosing(() => {
    snapshots.freeze()
    sessionsOfAgents.freeze()
  })

  // A terminal manager with no windows has nothing left to manage, so closing the
  // window quits rather than leaving the app resident the way macOS usually does.
  app.on('window-all-closed', () => {
    app.quit()
  })
}

// The shells need no teardown: each one exits when its pty master closes with
// the process.
function stopEverything(): void {
  for (const service of services) {
    service.stop()
  }
}

// The first before-quit is held back while the snapshot's final write runs; the
// app.quit() at the end of that write fires it a second time, which goes through.
app.on('before-quit', (event) => {
  if (flushed) {
    return
  }
  flushed = true

  if (snapshotStore === undefined) {
    stopEverything()
    return
  }

  event.preventDefault()
  snapshotStore.freeze()
  agentSessions.freeze()
  void snapshotStore.flushAtQuit().finally(() => {
    stopEverything()
    app.quit()
  })
})
