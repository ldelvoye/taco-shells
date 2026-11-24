import { type JSX, useCallback, useEffect, useMemo, useState } from 'react'
import { FOCUS_FIRST_PORT, FOCUS_LAST_PORT } from '@shared/focusPorts'
import { paletteFor } from '@shared/theme'
import type { Group, TabGroup } from '@shared/model'
import { waitingCount } from '@shared/rowState'
import { createCommandRegistry } from './commands/registry'
import { registerTerminalCommands } from './commands/terminal'
import { registerWorkspaceCommands } from './commands/workspace'
import { ConfigBanner } from './config/ConfigBanner'
import { applyChromePalette } from './config/palette'
import { useConfig } from './config/useConfig'
import { AgentBanners } from './agents/AgentBanners'
import { useAgentBanners } from './agents/useAgentBanners'
import { UpdateBanner } from './update/UpdateBanner'
import { useUpdate } from './update/useUpdate'
import { useCommandKeys } from './keys/useCommandKeys'
import { PaneArea } from './layout/PaneArea'
import { Sidebar } from './sidebar/Sidebar'
import { useFullScreen } from './sidebar/useFullScreen'
import { useSidebarWidth } from './sidebar/useSidebarWidth'
import { applyTerminalConfig } from './terminal/registry'
import { useWorkspace } from './workspace/useWorkspace'

const FOCUS_PORTS_TAKEN = `Ports ${FOCUS_FIRST_PORT}-${FOCUS_LAST_PORT} are all in use, so jumping to the terminal that wants you is off.`

export function App(): JSX.Element {
  const { state, operations } = useWorkspace()
  const workspace = state.workspace
  const agentRows = state.agentRows
  const [sidebarVisible, setSidebarVisible] = useState(true)
  const [focusProblem, setFocusProblem] = useState<string | null>(null)
  const commands = useMemo(() => createCommandRegistry(), [])
  const config = useConfig()
  const bindings = useMemo(() => new Map(config.bindings), [config.bindings])
  const sidebarWidth = useSidebarWidth()
  const fullScreen = useFullScreen()
  const update = useUpdate()
  const agentBanners = useAgentBanners()

  const applyUpdate = useCallback(() => {
    window.tacoShells.update.apply()
  }, [])

  const setUpAgent = useCallback((harness: string) => {
    window.tacoShells.agents.setUp(harness)
  }, [])

  const declineAgent = useCallback((harness: string) => {
    window.tacoShells.agents.decline(harness)
  }, [])

  const dismissAgent = useCallback((harness: string) => {
    window.tacoShells.agents.dismiss(harness)
  }, [])

  useEffect(() => {
    return window.tacoShells.focus.onUnavailable(() => {
      setFocusProblem(FOCUS_PORTS_TAKEN)
    })
  }, [])

  const toggleSidebar = useCallback(() => {
    setSidebarVisible((visible) => {
      return !visible
    })
  }, [])

  useEffect(() => {
    registerWorkspaceCommands(commands, {
      openTerminal: operations.openTerminal,
      splitTerminal: operations.splitActive,
      closeActiveTerminal: operations.closeActive,
      focusNextPane: () => {
        operations.moveFocus('nextPane')
      },
      focusPreviousPane: () => {
        operations.moveFocus('previousPane')
      },
      focusNextGroup: () => {
        operations.moveFocus('nextGroup')
      },
      focusPreviousGroup: () => {
        operations.moveFocus('previousGroup')
      },
      toggleActiveColor: operations.toggleActiveColor,
      toggleSidebar,
      openSettings: operations.openSettings,
      openKeybindings: operations.openKeybindings
    })
    registerTerminalCommands(commands, operations.activeSession)
  }, [commands, operations, toggleSidebar])

  useEffect(() => {
    const palette = paletteFor(config.appearance)
    applyChromePalette(palette)
  }, [config.appearance])

  useEffect(() => {
    const palette = paletteFor(config.appearance)
    applyTerminalConfig(config.settings, palette.terminal)
  }, [config])

  useCommandKeys(commands, bindings)

  let groups: Group[] = []
  let tabGroups: TabGroup[] = []
  let activeGroupIndex = 0
  if (workspace) {
    groups = workspace.groups
    tabGroups = workspace.tabGroups
    activeGroupIndex = workspace.activeGroup
  }

  let problems = config.problems
  if (focusProblem) {
    problems = [...config.problems, focusProblem]
  }

  let sidebar = null
  if (sidebarVisible) {
    sidebar = (
      <Sidebar
        groups={groups}
        activeGroup={activeGroupIndex}
        tabGroups={tabGroups}
        width={sidebarWidth.width}
        fullScreen={fullScreen}
        rowStates={agentRows}
        onSelect={operations.select}
        onClose={operations.close}
        onNewTerminal={operations.openTerminalIn}
        onDrop={operations.drop}
        onColor={operations.colorPane}
        onFork={operations.fork}
        onNewTabGroup={operations.newTabGroup}
        onRenameTabGroup={operations.renameTabGroup}
        onRecolorTabGroup={operations.recolorTabGroup}
        onUngroupTabGroup={operations.ungroupTabGroup}
        onToggleTabGroup={operations.toggleTabGroup}
        onRefocus={operations.refocusActive}
        onResize={sidebarWidth.resize}
      />
    )
  }

  // Hiding the sidebar takes the traffic lights' strip with it, so the pane area
  // grows its own. It is the only place a frameless window can be dragged by.
  let paneTitlebar = null
  if (!sidebarVisible) {
    const count = waitingCount(agentRows)
    let pill = null
    if (count > 0) {
      pill = (
        <div className="waiting-pill">
          <span className="waiting-pill-dot" />
          {`${count} waiting`}
        </div>
      )
    }
    paneTitlebar = <div className="pane-titlebar">{pill}</div>
  }

  return (
    <div className="app">
      {sidebar}
      <div className="pane-column">
        {paneTitlebar}
        <ConfigBanner problems={problems} />
        <UpdateBanner version={update.ready} problem={update.problem} onApply={applyUpdate} />
        <AgentBanners
          banners={agentBanners}
          onSetUp={setUpAgent}
          onDecline={declineAgent}
          onDismiss={dismissAgent}
        />
        <PaneArea groups={groups} activeGroup={activeGroupIndex} onResize={operations.resizeGroup} />
      </div>
    </div>
  )
}
