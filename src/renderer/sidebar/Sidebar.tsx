import {
  type CSSProperties,
  type DragEvent,
  type JSX,
  type MouseEvent,
  useCallback,
  useRef,
  useState
} from 'react'
import type { SessionId } from '@shared/ipc'
import { type DropPoint, dropFor, rowPointIn } from '@shared/drop'
import {
  blockOf,
  type Dragged,
  type Drop,
  type Group,
  groupHasRoom,
  type Pane,
  paneOf,
  type RowColor,
  type TabGroup,
  type TabGroupId,
  type Workspace
} from '@shared/model'
import { type ClaudeRows, summaryState } from '@shared/rowState'
import { RowColorPicker, rowColorVariable } from './RowColorPicker'
import { SidebarDivider } from './SidebarDivider'
import { SidebarTitlebar } from './SidebarTitlebar'
import { TabGroupPopup } from './TabGroupPopup'

const UNTITLED_ROW = 'Terminal'
const UNNAMED_TAB_GROUP = 'Group'

const CONNECTOR_FIRST = 'is-first'
const CONNECTOR_MIDDLE = 'is-middle'
const CONNECTOR_LAST = 'is-last'

const RIGHT_BUTTON = 2

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

type MarkTarget =
  | { on: 'entry'; index: number }
  | { on: 'wrapper'; tabGroup: TabGroupId }
  | { on: 'header'; tabGroup: TabGroupId }

interface DropMark {
  target: MarkTarget
  edge: 'above' | 'below' | 'whole'
  tabGroup: TabGroupId | null
}

interface SidebarProps {
  groups: Group[]
  activeGroup: number
  tabGroups: TabGroup[]
  width: number
  fullScreen: boolean
  rowStates: ClaudeRows
  onSelect: (sessionId: SessionId) => void
  onDrop: (dragged: Dragged, drop: Drop) => void
  onColor: (sessionId: SessionId, color: RowColor | null) => void
  onFork: (sessionId: SessionId) => void
  onNewTabGroup: (pane: SessionId) => TabGroupId
  onRenameTabGroup: (id: TabGroupId, name: string) => void
  onRecolorTabGroup: (id: TabGroupId, color: RowColor) => void
  onUngroupTabGroup: (id: TabGroupId) => void
  onToggleTabGroup: (id: TabGroupId) => void
  onRefocus: () => void
  onResize: (width: number) => void
}

// A row forks when it runs a Claude session with a conversation to copy and its
// group has room for the fork.
function canForkRow(pane: Pane, groups: Group[], rowStates: ClaudeRows): boolean {
  const row = rowStates.get(pane.id)
  if (row === undefined) {
    return false
  }
  if (!row.hasConversation) {
    return false
  }

  const group = groups.find((candidate) => candidate.panes.includes(pane))
  if (group === undefined) {
    return false
  }
  return groupHasRoom(group)
}

function sameDrop(a: Drop | null, b: Drop | null): boolean {
  if (a === null || b === null) {
    return a === b
  }
  if (a.kind === 'between' && b.kind === 'between') {
    return a.index === b.index && a.tabGroup === b.tabGroup
  }
  if (a.kind === 'into' && b.kind === 'into') {
    return a.group === b.group && a.slot === b.slot
  }
  return false
}

function markInNoGroup(workspace: Workspace, index: number): DropMark | null {
  const groups = workspace.groups
  if (index === groups.length) {
    const lastIndex = groups.length - 1
    const last = groups[lastIndex]
    if (last === undefined) {
      return null
    }
    if (last.tabGroup === null) {
      return { target: { on: 'entry', index: lastIndex }, edge: 'below', tabGroup: null }
    }
    return { target: { on: 'wrapper', tabGroup: last.tabGroup }, edge: 'below', tabGroup: null }
  }

  const entry = groups[index]
  if (entry.tabGroup !== null) {
    const block = blockOf(workspace, entry.tabGroup)
    if (block !== null && block.first === index) {
      return { target: { on: 'wrapper', tabGroup: entry.tabGroup }, edge: 'above', tabGroup: null }
    }
  }
  return { target: { on: 'entry', index }, edge: 'above', tabGroup: null }
}

function markInTabGroup(workspace: Workspace, id: TabGroupId, index: number): DropMark | null {
  const record = workspace.tabGroups.find((candidate) => candidate.id === id)
  const block = blockOf(workspace, id)
  if (record === undefined || block === null) {
    return null
  }

  if (record.collapsed) {
    return { target: { on: 'header', tabGroup: id }, edge: 'whole', tabGroup: id }
  }
  if (index === block.first) {
    return { target: { on: 'header', tabGroup: id }, edge: 'below', tabGroup: id }
  }
  if (index > block.last) {
    return { target: { on: 'entry', index: block.last }, edge: 'below', tabGroup: id }
  }
  return { target: { on: 'entry', index }, edge: 'above', tabGroup: id }
}

function dropMarkFor(workspace: Workspace, drop: Drop | null): DropMark | null {
  if (drop === null || drop.kind !== 'between') {
    return null
  }
  if (drop.tabGroup === null) {
    return markInNoGroup(workspace, drop.index)
  }
  return markInTabGroup(workspace, drop.tabGroup, drop.index)
}

function edgeClass(mark: DropMark): string {
  if (mark.edge === 'whole') {
    return ' is-drop-target'
  }

  let className = ' drop-above'
  if (mark.edge === 'below') {
    className = ' drop-below'
  }
  if (mark.tabGroup !== null) {
    className += ' drop-in-group'
  }
  return className
}

function entryMarkClass(mark: DropMark | null, index: number): string {
  if (mark === null) {
    return ''
  }
  const target = mark.target
  if (target.on !== 'entry' || target.index !== index) {
    return ''
  }
  return edgeClass(mark)
}

function wrapperMarkClass(mark: DropMark | null, id: TabGroupId): string {
  if (mark === null) {
    return ''
  }
  const target = mark.target
  if (target.on !== 'wrapper' || target.tabGroup !== id) {
    return ''
  }
  return edgeClass(mark)
}

function headerMarkClass(mark: DropMark | null, id: TabGroupId): string {
  if (mark === null) {
    return ''
  }
  const target = mark.target
  if (target.on !== 'header' || target.tabGroup !== id) {
    return ''
  }
  return edgeClass(mark)
}

function labelFor(title: string): string {
  if (title) {
    return title
  }
  return UNTITLED_ROW
}

function tabGroupLabel(name: string): string {
  if (name.trim()) {
    return name
  }
  return UNNAMED_TAB_GROUP
}

// CSS, not box-drawing glyphs: their em box is shorter than a row, so the
// strokes of adjacent rows would not quite meet.
function connectorFor(paneIndex: number, paneCount: number): string | null {
  if (paneCount < 2) {
    return null
  }
  if (paneIndex === 0) {
    return CONNECTOR_FIRST
  }
  if (paneIndex === paneCount - 1) {
    return CONNECTOR_LAST
  }
  return CONNECTOR_MIDDLE
}

export function Sidebar({
  groups,
  activeGroup,
  width,
  fullScreen,
  rowStates,
  tabGroups,
  onSelect,
  onDrop,
  onColor,
  onFork,
  onNewTabGroup,
  onRenameTabGroup,
  onRecolorTabGroup,
  onUngroupTabGroup,
  onToggleTabGroup,
  onRefocus,
  onResize
}: SidebarProps): JSX.Element {
  const dragged = useRef<Dragged | null>(null)
  const [drop, setDrop] = useState<Drop | null>(null)
  const [picker, setPicker] = useState<OpenPicker | null>(null)
  const [tabGroupPopup, setTabGroupPopup] = useState<OpenTabGroupPopup | null>(null)
  const workspace: Workspace = { groups, activeGroup, tabGroups }

  const closePicker = useCallback(() => {
    setPicker(null)
  }, [])

  const closeTabGroupPopup = useCallback(() => {
    setTabGroupPopup(null)
    onRefocus()
  }, [onRefocus])

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

  function startDrag(event: DragEvent<HTMLElement>, what: Dragged, label: string): void {
    dragged.current = what
    event.dataTransfer.effectAllowed = 'move'
    // The drop reads the ref above, not this. The label is what a drag that lands outside the sidebar drops.
    event.dataTransfer.setData('text/plain', label)
  }

  // The bracket is what was grabbed, but the picture that follows the pointer is the whole split.
  function showWholeGroup(event: DragEvent<HTMLElement>, selector: string): void {
    const shown = event.currentTarget.closest(selector)
    if (shown === null) {
      return
    }

    const bounds = shown.getBoundingClientRect()
    const offsetX = event.clientX - bounds.left
    const offsetY = event.clientY - bounds.top
    event.dataTransfer.setDragImage(shown, offsetX, offsetY)
  }

  function rowPointOf(event: DragEvent<HTMLDivElement>, groupIndex: number): DropPoint {
    const paneCount = groups[groupIndex].panes.length
    const bounds = event.currentTarget.getBoundingClientRect()
    const offsetY = event.clientY - bounds.top
    const point = rowPointIn(groupIndex, paneCount, offsetY, bounds.height)
    return { over: 'row', ...point }
  }

  function headerPointOf(event: DragEvent<HTMLDivElement>, tabGroup: TabGroupId): DropPoint {
    const bounds = event.currentTarget.getBoundingClientRect()
    const offsetY = event.clientY - bounds.top
    const raised = Math.max(offsetY / bounds.height, 0)
    const fraction = Math.min(raised, 1)
    return { over: 'header', tabGroup, fraction }
  }

  function dropAt(point: DropPoint): Drop | null {
    const dragging = dragged.current
    if (dragging === null) {
      return null
    }
    return dropFor(workspace, dragging, point)
  }

  function onDragOver(event: DragEvent<HTMLDivElement>, point: DropPoint): void {
    const next = dropAt(point)
    if (next === null) {
      setDrop(null)
      return
    }

    event.preventDefault()
    event.dataTransfer.dropEffect = 'move'
    if (!sameDrop(drop, next)) {
      setDrop(next)
    }
  }

  function onDropAt(event: DragEvent<HTMLDivElement>, point: DropPoint): void {
    event.preventDefault()

    const landed = dropAt(point)
    const draggedValue = dragged.current
    endDrag()

    if (draggedValue === null || landed === null) {
      return
    }
    onDrop(draggedValue, landed)
  }

  function endDrag(): void {
    dragged.current = null
    setDrop(null)
  }

  // The empty space under the last row drops at the bottom of the list, in no group.
  function onDragOverEnd(event: DragEvent<HTMLDivElement>): void {
    if (event.target === event.currentTarget) {
      onDragOver(event, END_POINT)
    }
  }

  function onDropOnEnd(event: DragEvent<HTMLDivElement>): void {
    if (event.target === event.currentTarget) {
      onDropAt(event, END_POINT)
    }
  }

  // A dragleave also fires when the pointer moves between two rows, so only one that ends outside the rows counts.
  function onDragLeaveRows(event: DragEvent<HTMLDivElement>): void {
    const bounds = event.currentTarget.getBoundingClientRect()
    const insideAcross = event.clientX >= bounds.left && event.clientX < bounds.right
    const insideDown = event.clientY >= bounds.top && event.clientY < bounds.bottom
    if (insideAcross && insideDown) {
      return
    }
    setDrop(null)
  }

  const mark = dropMarkFor(workspace, drop)

  const entries = groups.map((group, groupIndex) => {
    const paneCount = group.panes.length
    const activeInGroup = paneOf(group)
    const groupDragLabel = labelFor(activeInGroup.title)

    let joinSlot: number | null = null
    if (drop !== null && drop.kind === 'into' && drop.group === group.id) {
      joinSlot = drop.slot
    }

    const rows = group.panes.map((pane, paneIndex) => {
      const label = labelFor(pane.title)

      let connector = null
      const connectorPiece = connectorFor(paneIndex, paneCount)
      if (connectorPiece) {
        connector = (
          <span
            className={`row-connector ${connectorPiece}`}
            draggable
            onDragStart={(event) => {
              event.stopPropagation()
              startDrag(event, { kind: 'group', group: group.id }, groupDragLabel)
              showWholeGroup(event, '.terminal-group')
            }}
          />
        )
      }

      let markerState: string | undefined
      const claudeRow = rowStates.get(pane.id)
      if (claudeRow !== undefined) {
        markerState = claudeRow.state
      }

      let className = 'terminal-row'
      const isActivePane = groupIndex === activeGroup && paneIndex === group.activePane
      if (isActivePane) {
        className += ' is-active'
      }
      if (joinSlot === paneIndex) {
        className += ' drop-before'
      }
      if (joinSlot === paneCount && paneIndex === paneCount - 1) {
        className += ' drop-after'
      }

      let stripe = null
      const color = shownColor(pane)
      if (color !== null) {
        const background = rowColorVariable(color)
        stripe = <span className="row-color" data-color={color} style={{ background }} />
      }

      return (
        <div
          key={pane.id}
          className={className}
          title={label}
          draggable
          onDragStart={(event) => {
            startDrag(event, { kind: 'pane', pane: pane.id }, label)
          }}
          onClick={() => {
            onSelect(pane.id)
          }}
          onMouseDown={(event) => {
            // Keeps keyboard focus in the terminal: right-clicking a row only opens the picker.
            if (event.button === RIGHT_BUTTON) {
              event.preventDefault()
            }
          }}
          onContextMenu={(event) => {
            openPicker(event, pane.id)
          }}
        >
          {stripe}
          {connector}
          <span className="row-state" data-state={markerState} />
          <span className="row-label">{label}</span>
        </div>
      )
    })

    let className = 'terminal-group'
    className += entryMarkClass(mark, groupIndex)
    if (joinSlot !== null) {
      className += ' is-join-target'
    }

    // A row drags its one terminal and the bracket drags the whole split, so the group only receives drops.
    return (
      <div
        key={group.id}
        className={className}
        onDragOver={(event) => {
          const point = rowPointOf(event, groupIndex)
          onDragOver(event, point)
        }}
        onDrop={(event) => {
          const point = rowPointOf(event, groupIndex)
          onDropAt(event, point)
        }}
        onDragEnd={endDrag}
      >
        {rows}
      </div>
    )
  })

  function renderTabGroup(tabGroup: TabGroup, first: number, last: number): JSX.Element {
    const members = groups.slice(first, last + 1)
    let shownEntries: JSX.Element[] = []
    let className = 'tab-group'
    let headerClassName = 'tab-group-header'
    let count = null
    if (tabGroup.collapsed) {
      className += ' is-collapsed'
      const memberPanes = members.flatMap((member) => member.panes)
      const memberIds = memberPanes.map((pane) => pane.id)
      const summary = summaryState(rowStates, memberIds)
      let marker = null
      if (summary !== null) {
        marker = <span className="row-state" data-state={summary} />
      }
      count = (
        <>
          <span className="tab-group-count">{memberPanes.length}</span>
          {marker}
        </>
      )
      if (activeGroup >= first && activeGroup <= last) {
        headerClassName += ' is-active'
      }
    } else {
      shownEntries = entries.slice(first, last + 1)
    }

    className += wrapperMarkClass(mark, tabGroup.id)
    headerClassName += headerMarkClass(mark, tabGroup.id)

    const tabStyle = { '--tab-color': rowColorVariable(tabGroup.color) } as CSSProperties
    const dragLabel = tabGroupLabel(tabGroup.name)
    return (
      <div key={tabGroup.id} className={className} data-tab-group={tabGroup.id} style={tabStyle}>
        <div
          className={headerClassName}
          draggable
          onDragStart={(event) => {
            startDrag(event, { kind: 'tabGroup', tabGroup: tabGroup.id }, dragLabel)
            showWholeGroup(event, '.tab-group')
          }}
          onDragOver={(event) => {
            const point = headerPointOf(event, tabGroup.id)
            onDragOver(event, point)
          }}
          onDrop={(event) => {
            const point = headerPointOf(event, tabGroup.id)
            onDropAt(event, point)
          }}
          onDragEnd={endDrag}
          onClick={() => {
            onToggleTabGroup(tabGroup.id)
          }}
          onMouseDown={(event) => {
            if (event.button === RIGHT_BUTTON) {
              event.preventDefault()
            }
          }}
          onContextMenu={(event) => {
            openTabGroupPopup(event, tabGroup.id)
          }}
        >
          <span className="tab-group-caret" />
          <span className="tab-group-name">{tabGroupLabel(tabGroup.name)}</span>
          {count}
        </div>
        {shownEntries}
      </div>
    )
  }

  const rendered: JSX.Element[] = []
  let walkIndex = 0
  while (walkIndex < groups.length) {
    const entryTabGroup = groups[walkIndex].tabGroup
    let tabGroup: TabGroup | undefined
    let block = null
    if (entryTabGroup !== null) {
      tabGroup = tabGroups.find((candidate) => candidate.id === entryTabGroup)
      block = blockOf(workspace, entryTabGroup)
    }

    if (tabGroup === undefined || block === null) {
      rendered.push(entries[walkIndex])
      walkIndex += 1
    } else {
      rendered.push(renderTabGroup(tabGroup, block.first, block.last))
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
      const canFork = canForkRow(pickedPane, groups, rowStates)
      colorPicker = (
        <RowColorPicker
          x={picker.x}
          y={picker.y}
          current={pickedPane.color}
          canFork={canFork}
          onPreview={(color) => {
            setPicker({ ...picker, preview: { color } })
          }}
          onPreviewEnd={() => {
            setPicker({ ...picker, preview: null })
          }}
          onPick={(color) => {
            onColor(pickedPane.id, color)
            closePicker()
          }}
          onFork={() => {
            onFork(pickedPane.id)
            closePicker()
          }}
          onNewGroup={() => {
            const tabGroup = onNewTabGroup(pickedPane.id)
            closePicker()
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
            onRenameTabGroup(popupTabGroup.id, name)
          }}
          onRecolor={(color) => {
            onRecolorTabGroup(popupTabGroup.id, color)
          }}
          onUngroup={() => {
            onUngroupTabGroup(popupTabGroup.id)
          }}
          onClose={closeTabGroupPopup}
        />
      )
    }
  }

  return (
    <aside className="sidebar" style={style}>
      <SidebarTitlebar fullScreen={fullScreen} />
      <div
        className="sidebar-rows"
        onDragOver={onDragOverEnd}
        onDrop={onDropOnEnd}
        onDragLeave={onDragLeaveRows}
      >
        {rendered}
      </div>
      <SidebarDivider width={width} onResize={onResize} />
      {colorPicker}
      {groupPopup}
    </aside>
  )
}
