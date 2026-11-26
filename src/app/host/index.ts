import { app, net, shell } from 'electron'
import type { ConfigFile } from '@protocol/config'
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
import { startRuntimeProcess } from './runtime/process'
import { nativeSystemTheme } from './systemTheme'
import { ApplicationMenu } from './window/applicationMenu'
import { linkTo, type WindowLink } from './window/link'
import { createMainWindow, isBackgroundWindow } from './window/window'

app.setName(applicationName)

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

  const runtimeProcess = startRuntimeProcess()
  const runtime = runtimeProcess.runtime
  const directory = configDirectory()
  const configStore = new ConfigStore(directory, nativeSystemTheme())
  const initialConfig = configStore.current()
  const palette = paletteFor(initialConfig.appearance)
  const link = linkTo(createMainWindow(palette))
  const snapshots = new SnapshotStore(directory, agentSessions, (id) => runtime.directoryOf(id))
  snapshotStore = snapshots
  const restoreOnLaunch = initialConfig.settings.restoreOnLaunch
  const configPathOf = (file: ConfigFile): string => configFilePath(directory, file)
  const workspaceBridge = bridgeWorkspace(
    link,
    snapshots,
    agentSessions,
    runtime,
    configPathOf,
    restoreOnLaunch
  )
  const workspace = workspaceBridge.workspace
  // What is saved is frozen first, so it still holds the terminals as they were:
  // quitting and reopening then brings them back.
  runtimeProcess.onDied((reason) => {
    freezeWhatIsSaved(snapshots)
    workspace.runtimeDied(reason)
  })
  registerIpc(runtimeProcess, link, configStore)
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
    return startAgents(directory, configStore, runtime, agentsHost, events)
  })

  const focus = bridgeFocus(link, workspace, (events) => {
    return startFocus(runtime, directory,events, () => {
      updates.checkNow()
    })
  })
  // In the order they stop, the runtime process last.
  services.push(configStore, workspaceBridge, focus, updates, agents, snapshots, runtimeProcess)

  whenTheWindowCloses(link, snapshots)

  configStore.onChange((config) => {
    const nextPalette = paletteFor(config.appearance)
    link.setBackgroundColor(nextPalette.chrome.terminalBackground)
    runtime.setScrollback(config.settings.scrollback)
    runtime.setColors(nextPalette.terminal)
  })

  runtime.setScrollback(initialConfig.settings.scrollback)
  runtime.setColors(palette.terminal)
  // Last, so everything that listens to a shell is in place before the first one starts.
  workspace.start()
})

// The save file is made from the snapshot store and the agent sessions, and both stop together.
function freezeWhatIsSaved(snapshots: SnapshotStore): void {
  snapshots.freeze()
  agentSessions.freeze()
}

// The one place that decides what the window closing means: it ends the app.
function whenTheWindowCloses(link: WindowLink, snapshots: SnapshotStore): void {
  // Closing the window is the start of a quit, and the snapshot has to stop
  // changing from that moment, before the runtime process is stopped.
  link.onClosing(() => {
    freezeWhatIsSaved(snapshots)
  })

  // A terminal manager with no windows has nothing left to manage, so closing the
  // window quits rather than leaving the app resident the way macOS usually does.
  app.on('window-all-closed', () => {
    app.quit()
  })
}

// The shells need no teardown: each one exits when its pty master closes with
// the runtime process, which stops last of the services.
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
  freezeWhatIsSaved(snapshotStore)
  void snapshotStore.flushAtQuit().finally(() => {
    stopEverything()
    app.quit()
  })
})
