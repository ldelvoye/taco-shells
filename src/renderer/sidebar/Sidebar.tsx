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
import { type Group, type Pane, paneOf, type RowColor } from '@shared/model'
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
  onReorder: (from: number, to: number) => void
  onColor: (sessionId: SessionId, color: RowColor | null) => void
  onResize: (width: number) => void
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
  onReorder,
  onColor,
  onResize
}: SidebarProps): JSX.Element {
  const dragged = useRef<number | null>(null)
  const [insertAt, setInsertAt] = useState<number | null>(null)
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

  function onDragStart(event: DragEvent<HTMLDivElement>, index: number, label: string): void {
    dragged.current = index
    event.dataTransfer.effectAllowed = 'move'
    // The reorder reads the index above, not this. The group carries its name so
    // a drag that lands outside the sidebar drops something meaningful.
    event.dataTransfer.setData('text/plain', label)
  }

  function onDragOver(event: DragEvent<HTMLDivElement>, index: number): void {
    if (dragged.current === null) {
      return
    }

    event.preventDefault()
    event.dataTransfer.dropEffect = 'move'

    const bounds = event.currentTarget.getBoundingClientRect()
    const offsetY = event.clientY - bounds.top
    if (offsetY > bounds.height / 2) {
      setInsertAt(index + 1)
    } else {
      setInsertAt(index)
    }
  }

  function onDrop(event: DragEvent<HTMLDivElement>): void {
    event.preventDefault()

    const from = dragged.current
    const target = insertAt
    endDrag()

    if (from === null || target === null) {
      return
    }

    // The drop point is a gap between groups, so it counts the dragged group
    // itself whenever that group sits above the gap.
    let to = target
    if (from < target) {
      to = target - 1
    }
    onReorder(from, to)
  }

  function endDrag(): void {
    dragged.current = null
    setInsertAt(null)
  }

  const lastIndex = groups.length - 1
  const rendered = groups.map((group, groupIndex) => {
    const paneCount = group.panes.length
    const rows = group.panes.map((pane, paneIndex) => {
      const label = labelFor(pane.title)

      let connector = null
      const connectorPiece = connectorFor(paneIndex, paneCount)
      if (connectorPiece) {
        connector = <span className={`row-connector ${connectorPiece}`} />
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

    // Dragging is a group-level gesture, so the group is what carries the drag
    // handlers and the drop marker: its panes travel with it as one unit.
    let className = 'terminal-group'
    if (insertAt === groupIndex) {
      className += ' drop-above'
    }
    if (groupIndex === lastIndex && insertAt === groups.length) {
      className += ' drop-below'
    }

    const shownWhileDragged = paneOf(group)
    const dragLabel = labelFor(shownWhileDragged.title)

    return (
      <div
        key={group.id}
        className={className}
        draggable
        onDragStart={(event) => {
          onDragStart(event, groupIndex, dragLabel)
        }}
        onDragOver={(event) => {
          onDragOver(event, groupIndex)
        }}
        onDrop={onDrop}
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
      colorPicker = (
        <RowColorPicker
          x={picker.x}
          y={picker.y}
          current={pickedPane.color}
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
