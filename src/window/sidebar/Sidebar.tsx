import {
  type CSSProperties,
  type DragEvent,
  type JSX,
  type MouseEvent,
  useCallback,
  useEffect,
  useState
} from 'react'
import { DEFAULT_HARNESS, harnessOf } from '@agents/registry'
import type {
  Group,
  Pane,
  RowColor,
  TabGroup,
  TabGroupId,
  Workspace
} from '@protocol/arrangement'
import type { SessionId } from '@protocol/ipc'
import type { AgentRows } from '@protocol/rows'
import { blockOf, groupHasRoom } from '@rules/arrangement'
import type { DropPoint } from '@rules/drop'
import { RowPopup } from './RowPopup'
import type { SidebarActions } from './sidebarActions'
import { SidebarDivider } from './SidebarDivider'
import { SidebarEntry } from './SidebarEntry'
import { SidebarTitlebar } from './SidebarTitlebar'
import { TabGroupPopup } from './TabGroupPopup'
import { TabGroupSection } from './TabGroupSection'
import { useSidebarDrag } from './useSidebarDrag'

const END_POINT: DropPoint = { over: 'end' }

// Wrapped because previewing no color is different from not previewing at all.
interface ColorPreview {
  color: RowColor | null
}

interface OpenRowPopup {
  paneId: SessionId
  x: number
  y: number
  preview: ColorPreview | null
}

interface OpenTabGroupPopup {
  tabGroup: TabGroupId
  x: number
  y: number
}

interface SidebarProps {
  workspace: Workspace
  agentRows: AgentRows
  actions: SidebarActions
  width: number
  fullScreen: boolean
  onResize: (width: number) => void
}

// A row forks when its agent can fork, its session has a conversation to copy and its
// group has room for the fork.
function canForkRow(pane: Pane, groups: Group[], rowStates: AgentRows): boolean {
  const row = rowStates.get(pane.id)
  if (row === undefined) {
    return false
  }
  if (!row.hasConversation) {
    return false
  }
  const harness = harnessOf(row.harness)
  if (harness === null || harness.forkCommand === undefined) {
    return false
  }

  const group = groups.find((candidate) => candidate.panes.includes(pane))
  if (group === undefined) {
    return false
  }
  return groupHasRoom(group)
}

function harnessNameOf(pane: Pane, rowStates: AgentRows): string {
  const row = rowStates.get(pane.id)
  if (row === undefined) {
    return DEFAULT_HARNESS.name
  }
  const harness = harnessOf(row.harness)
  if (harness === null) {
    return DEFAULT_HARNESS.name
  }
  return harness.name
}

export function Sidebar({
  workspace,
  agentRows,
  actions,
  width,
  fullScreen,
  onResize
}: SidebarProps): JSX.Element {
  const drag = useSidebarDrag(workspace, actions.drop)
  const [rowPopup, setRowPopup] = useState<OpenRowPopup | null>(null)
  const [tabGroupPopup, setTabGroupPopup] = useState<OpenTabGroupPopup | null>(null)
  const groups = workspace.groups
  const activeGroup = workspace.activeGroup
  const tabGroups = workspace.tabGroups

  const closeRowPopup = useCallback(() => {
    setRowPopup(null)
  }, [])

  // The name field took the keyboard, so closing its popup hands it back to the terminal.
  const closeTabGroupPopup = useCallback(() => {
    setTabGroupPopup(null)
    actions.refocusActive()
  }, [actions])

  // A tab group can vanish under its open popup, when its last terminal exits.
  useEffect(() => {
    if (tabGroupPopup === null) {
      return
    }
    const stillThere = tabGroups.some((candidate) => candidate.id === tabGroupPopup.tabGroup)
    if (stillThere) {
      return
    }
    closeTabGroupPopup()
  }, [tabGroupPopup, tabGroups, closeTabGroupPopup])

  function openTabGroupPopup(event: MouseEvent<HTMLDivElement>, tabGroup: TabGroupId): void {
    event.preventDefault()
    setTabGroupPopup({ tabGroup, x: event.clientX, y: event.clientY })
  }

  function openRowPopup(event: MouseEvent<HTMLDivElement>, paneId: SessionId): void {
    event.preventDefault()
    setRowPopup({ paneId, x: event.clientX, y: event.clientY, preview: null })
  }

  function shownColor(pane: Pane): RowColor | null {
    if (rowPopup === null || rowPopup.paneId !== pane.id || rowPopup.preview === null) {
      return pane.color
    }
    return rowPopup.preview.color
  }

  // The empty space under the last row drops at the bottom of the list, in no group.
  function onDragOverEnd(event: DragEvent<HTMLDivElement>): void {
    if (event.target === event.currentTarget) {
      drag.over(event, END_POINT)
    }
  }

  function onDropOnEnd(event: DragEvent<HTMLDivElement>): void {
    if (event.target === event.currentTarget) {
      drag.dropAt(event, END_POINT)
    }
  }

  function openUngroupedTerminal(): void {
    actions.openTerminalIn(null)
  }

  const rendered: JSX.Element[] = []
  let walkIndex = 0
  while (walkIndex < groups.length) {
    const group = groups[walkIndex]
    const entryTabGroup = group.tabGroup
    let tabGroup: TabGroup | undefined
    let block = null
    if (entryTabGroup !== null) {
      tabGroup = tabGroups.find((candidate) => candidate.id === entryTabGroup)
      block = blockOf(workspace, entryTabGroup)
    }

    if (tabGroup === undefined || block === null) {
      const entry = (
        <SidebarEntry
          key={group.id}
          group={group}
          groupIndex={walkIndex}
          activeGroup={activeGroup}
          agentRows={agentRows}
          shownColor={shownColor}
          drag={drag}
          actions={actions}
          onOpenRowPopup={openRowPopup}
        />
      )
      rendered.push(entry)
      walkIndex += 1
    } else {
      const section = (
        <TabGroupSection
          key={tabGroup.id}
          tabGroup={tabGroup}
          groups={groups}
          first={block.first}
          last={block.last}
          activeGroup={activeGroup}
          agentRows={agentRows}
          shownColor={shownColor}
          drag={drag}
          actions={actions}
          onOpenRowPopup={openRowPopup}
          onOpenPopup={openTabGroupPopup}
        />
      )
      rendered.push(section)
      walkIndex = block.last + 1
    }
  }

  const style = { '--sidebar-width': `${width}px` } as CSSProperties

  // A pane can exit while its popup is open, which takes the popup with it.
  let rowPopupElement = null
  if (rowPopup !== null) {
    const panes = groups.flatMap((group) => group.panes)
    const pickedPane = panes.find((pane) => pane.id === rowPopup.paneId)
    if (pickedPane !== undefined) {
      const canFork = canForkRow(pickedPane, groups, agentRows)
      const forkName = harnessNameOf(pickedPane, agentRows)
      rowPopupElement = (
        <RowPopup
          x={rowPopup.x}
          y={rowPopup.y}
          current={pickedPane.color}
          canFork={canFork}
          forkName={forkName}
          onPreview={(color) => {
            setRowPopup({ ...rowPopup, preview: { color } })
          }}
          onPreviewEnd={() => {
            setRowPopup({ ...rowPopup, preview: null })
          }}
          onPick={(color) => {
            actions.colorPane(pickedPane.id, color)
            closeRowPopup()
          }}
          onFork={() => {
            actions.fork(pickedPane.id)
            closeRowPopup()
          }}
          onNewGroup={() => {
            const tabGroup = actions.newTabGroup(pickedPane.id)
            closeRowPopup()
            if (tabGroup === null) {
              return
            }
            setTabGroupPopup({ tabGroup, x: rowPopup.x, y: rowPopup.y })
          }}
          onClose={closeRowPopup}
        />
      )
    }
  }

  let groupPopup = null
  if (tabGroupPopup !== null) {
    const popupTabGroup = tabGroups.find((candidate) => candidate.id === tabGroupPopup.tabGroup)
    if (popupTabGroup !== undefined) {
      groupPopup = (
        <TabGroupPopup
          x={tabGroupPopup.x}
          y={tabGroupPopup.y}
          name={popupTabGroup.name}
          color={popupTabGroup.color}
          onRename={(name) => {
            actions.renameTabGroup(popupTabGroup.id, name)
          }}
          onRecolor={(color) => {
            actions.recolorTabGroup(popupTabGroup.id, color)
          }}
          onUngroup={() => {
            actions.ungroupTabGroup(popupTabGroup.id)
          }}
          onClose={closeTabGroupPopup}
        />
      )
    }
  }

  return (
    <aside className="sidebar" style={style}>
      <SidebarTitlebar fullScreen={fullScreen} onNewTerminal={openUngroupedTerminal} />
      <div
        className="sidebar-rows"
        onDragOver={onDragOverEnd}
        onDrop={onDropOnEnd}
        onDragLeave={drag.leaveRows}
      >
        {rendered}
      </div>
      <SidebarDivider width={width} onResize={onResize} />
      {rowPopupElement}
      {groupPopup}
    </aside>
  )
}
