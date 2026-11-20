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
import { dropFor, rowPointIn } from '@shared/drop'
import {
  type Dragged,
  type Drop,
  type Group,
  groupHasRoom,
  type Pane,
  paneOf,
  type RowColor,
  type Workspace
} from '@shared/model'
import type { ClaudeRows } from '@shared/rowState'
import { RowColorPicker, rowColorVariable } from './RowColorPicker'
import { SidebarDivider } from './SidebarDivider'
import { SidebarTitlebar } from './SidebarTitlebar'

const UNTITLED_ROW = 'Terminal'

const CONNECTOR_FIRST = 'is-first'
const CONNECTOR_MIDDLE = 'is-middle'
const CONNECTOR_LAST = 'is-last'

const RIGHT_BUTTON = 2

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

interface SidebarProps {
  groups: Group[]
  activeGroup: number
  width: number
  fullScreen: boolean
  rowStates: ClaudeRows
  onSelect: (sessionId: SessionId) => void
  onDrop: (dragged: Dragged, drop: Drop) => void
  onColor: (sessionId: SessionId, color: RowColor | null) => void
  onFork: (sessionId: SessionId) => void
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
    return a.index === b.index
  }
  if (a.kind === 'into' && b.kind === 'into') {
    return a.group === b.group && a.slot === b.slot
  }
  return false
}

function labelFor(title: string): string {
  if (title) {
    return title
  }
  return UNTITLED_ROW
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
  onSelect,
  onDrop,
  onColor,
  onFork,
  onResize
}: SidebarProps): JSX.Element {
  const dragged = useRef<Dragged | null>(null)
  const [drop, setDrop] = useState<Drop | null>(null)
  const [picker, setPicker] = useState<OpenPicker | null>(null)

  const closePicker = useCallback(() => {
    setPicker(null)
  }, [])

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
  function showWholeGroup(event: DragEvent<HTMLElement>): void {
    const groupElement = event.currentTarget.closest('.terminal-group')
    if (groupElement === null) {
      return
    }

    const bounds = groupElement.getBoundingClientRect()
    const offsetX = event.clientX - bounds.left
    const offsetY = event.clientY - bounds.top
    event.dataTransfer.setDragImage(groupElement, offsetX, offsetY)
  }

  function dropAt(event: DragEvent<HTMLDivElement>, groupIndex: number): Drop | null {
    const dragging = dragged.current
    if (dragging === null) {
      return null
    }

    const paneCount = groups[groupIndex].panes.length
    const bounds = event.currentTarget.getBoundingClientRect()
    const offsetY = event.clientY - bounds.top
    const point = rowPointIn(groupIndex, paneCount, offsetY, bounds.height)
    const workspace: Workspace = { groups, activeGroup }
    return dropFor(workspace, dragging, point)
  }

  function onDragOver(event: DragEvent<HTMLDivElement>, groupIndex: number): void {
    const next = dropAt(event, groupIndex)
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

  function onDropOnGroup(event: DragEvent<HTMLDivElement>, groupIndex: number): void {
    event.preventDefault()

    const landed = dropAt(event, groupIndex)
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

  const lastIndex = groups.length - 1
  const rendered = groups.map((group, groupIndex) => {
    const paneCount = group.panes.length
    const shownWhileDragged = paneOf(group)
    const groupDragLabel = labelFor(shownWhileDragged.title)

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
              showWholeGroup(event)
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

    // A row drags its one terminal and the bracket drags the whole split, so the group only receives drops.
    let className = 'terminal-group'
    if (drop !== null && drop.kind === 'between' && drop.index === groupIndex) {
      className += ' drop-above'
    }
    if (drop !== null && drop.kind === 'between' && drop.index === groups.length) {
      if (groupIndex === lastIndex) {
        className += ' drop-below'
      }
    }
    if (joinSlot !== null) {
      className += ' is-join-target'
    }

    return (
      <div
        key={group.id}
        className={className}
        onDragOver={(event) => {
          onDragOver(event, groupIndex)
        }}
        onDrop={(event) => {
          onDropOnGroup(event, groupIndex)
        }}
        onDragEnd={endDrag}
      >
        {rows}
      </div>
    )
  })

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
          onClose={closePicker}
        />
      )
    }
  }

  return (
    <aside className="sidebar" style={style}>
      <SidebarTitlebar fullScreen={fullScreen} />
      <div className="sidebar-rows">{rendered}</div>
      <SidebarDivider width={width} onResize={onResize} />
      {colorPicker}
    </aside>
  )
}
