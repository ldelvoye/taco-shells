import { type DragEvent, type JSX, useRef, useState } from 'react'
import type { SessionId } from '@shared/ipc'
import { type Group, paneOf } from '@shared/model'

const UNTITLED_ROW = 'Terminal'

interface SidebarProps {
  groups: Group[]
  activeGroup: number
  onSelect: (index: number, sessionId: SessionId) => void
  onReorder: (from: number, to: number) => void
}

export function Sidebar({ groups, activeGroup, onSelect, onReorder }: SidebarProps): JSX.Element {
  const dragged = useRef<number | null>(null)
  const [insertAt, setInsertAt] = useState<number | null>(null)

  function onDragStart(event: DragEvent<HTMLDivElement>, index: number, label: string): void {
    dragged.current = index
    event.dataTransfer.effectAllowed = 'move'
    // The reorder reads the index above, not this. The row carries its name so a
    // drag that lands outside the sidebar drops something meaningful.
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

    // The drop point is a gap between rows, so it counts the dragged row itself
    // whenever that row sits above the gap.
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
  const rows = groups.map((group, index) => {
    const pane = paneOf(group)

    let label = pane.title
    if (!label) {
      label = UNTITLED_ROW
    }

    let className = 'terminal-row'
    if (index === activeGroup) {
      className += ' is-active'
    }
    if (insertAt === index) {
      className += ' drop-above'
    }
    if (index === lastIndex && insertAt === groups.length) {
      className += ' drop-below'
    }

    return (
      <div
        key={group.id}
        className={className}
        draggable
        title={label}
        onClick={() => {
          onSelect(index, pane.id)
        }}
        onDragStart={(event) => {
          onDragStart(event, index, label)
        }}
        onDragOver={(event) => {
          onDragOver(event, index)
        }}
        onDrop={onDrop}
        onDragEnd={endDrag}
      >
        {label}
      </div>
    )
  })

  return (
    <aside className="sidebar">
      <div className="sidebar-titlebar" />
      <div className="sidebar-rows">{rows}</div>
    </aside>
  )
}
