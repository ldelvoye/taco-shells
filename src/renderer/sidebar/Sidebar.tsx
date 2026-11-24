import {
  type CSSProperties,
  type DragEvent,
  type JSX,
  type MouseEvent,
  useCallback,
  useEffect,
  useState
} from 'react'
import type { SessionId } from '@shared/ipc'
import type { DropPoint } from '@shared/drop'
import {
  blockOf,
  type Group,
  groupHasRoom,
  type Pane,
  type RowColor,
  type TabGroup,
  type TabGroupId,
  type Workspace
} from '@shared/model'
import { DEFAULT_HARNESS, harnessOf } from '@shared/harness/registry'
import type { AgentRows } from '@shared/rowState'
import { RowColorPicker } from './RowColorPicker'
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

interface OpenPicker {
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
  const [picker, setPicker] = useState<OpenPicker | null>(null)
  const [tabGroupPopup, setTabGroupPopup] = useState<OpenTabGroupPopup | null>(null)
  const groups = workspace.groups
  const activeGroup = workspace.activeGroup
  const tabGroups = workspace.tabGroups

  const closePicker = useCallback(() => {
    setPicker(null)
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

  function openPicker(event: MouseEvent<HTMLDivElement>, paneId: SessionId): void {
    event.preventDefault()
    setPicker({ paneId, x: event.clientX, y: event.clientY, preview: null })
  }

  function shownColor(pane: Pane): RowColor | null {
    if (picker === null || picker.paneId !== pane.id || picker.preview === null) {
      return pane.color
    }
    return picker.preview.color
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
          onOpenPicker={openPicker}
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
          onOpenPicker={openPicker}
          onOpenPopup={openTabGroupPopup}
        />
      )
      rendered.push(section)
      walkIndex = block.last + 1
    }
  }

  const style = { '--sidebar-width': `${width}px` } as CSSProperties

  // A pane can exit while its picker is open, which takes the picker with it.
  let colorPicker = null
  if (picker !== null) {
    const panes = groups.flatMap((group) => group.panes)
    const pickedPane = panes.find((pane) => pane.id === picker.paneId)
    if (pickedPane !== undefined) {
      const canFork = canForkRow(pickedPane, groups, agentRows)
      const forkName = harnessNameOf(pickedPane, agentRows)
      colorPicker = (
        <RowColorPicker
          x={picker.x}
          y={picker.y}
          current={pickedPane.color}
          canFork={canFork}
          forkName={forkName}
          onPreview={(color) => {
            setPicker({ ...picker, preview: { color } })
          }}
          onPreviewEnd={() => {
            setPicker({ ...picker, preview: null })
          }}
          onPick={(color) => {
            actions.colorPane(pickedPane.id, color)
            closePicker()
          }}
          onFork={() => {
            actions.fork(pickedPane.id)
            closePicker()
          }}
          onNewGroup={() => {
            const tabGroup = actions.newTabGroup(pickedPane.id)
            closePicker()
            if (tabGroup === null) {
              return
            }
            setTabGroupPopup({ tabGroup, x: picker.x, y: picker.y })
          }}
          onClose={closePicker}
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
      {colorPicker}
      {groupPopup}
    </aside>
  )
}
