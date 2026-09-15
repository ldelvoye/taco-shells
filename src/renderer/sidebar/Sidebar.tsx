import { type CSSProperties, type DragEvent, type JSX, useRef, useState } from 'react'
import type { SessionId } from '@shared/ipc'
import { type Group, paneOf } from '@shared/model'
import { SidebarDivider } from './SidebarDivider'

const UNTITLED_ROW = 'Terminal'

const CONNECTOR_FIRST = 'is-first'
const CONNECTOR_MIDDLE = 'is-middle'
const CONNECTOR_LAST = 'is-last'

interface SidebarProps {
  groups: Group[]
  activeGroup: number
  width: number
  onSelect: (sessionId: SessionId) => void
  onReorder: (from: number, to: number) => void
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
  onSelect,
  onReorder,
  onResize
}: SidebarProps): JSX.Element {
  const dragged = useRef<number | null>(null)
  const [insertAt, setInsertAt] = useState<number | null>(null)

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

      let className = 'terminal-row'
      const isActivePane = groupIndex === activeGroup && paneIndex === group.activePane
      if (isActivePane) {
        className += ' is-active'
      }

      return (
        <div
          key={pane.id}
          className={className}
          title={label}
          onClick={() => {
            onSelect(pane.id)
          }}
        >
          {connector}
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

  return (
    <aside className="sidebar" style={style}>
      <div className="sidebar-titlebar" />
      <div className="sidebar-rows">{rendered}</div>
      <SidebarDivider width={width} onResize={onResize} />
    </aside>
  )
}
