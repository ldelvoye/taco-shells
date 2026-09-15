import { type JSX, type MouseEvent, type PointerEvent, useRef } from 'react'

interface DragOrigin {
  pointerX: number
  width: number
}

interface SidebarDividerProps {
  width: number
  onResize: (width: number) => void
}

const ROW_LABEL_SELECTOR = '.row-label'
const TERMINAL_ROW_SELECTOR = '.terminal-row'

// scrollWidth is rounded, so it can under-report and leave the fit a fraction
// short with the label still ellipsised. A range measures the text unrounded.
function textWidthOf(label: HTMLElement): number {
  const range = document.createRange()
  range.selectNodeContents(label)
  return range.getBoundingClientRect().width
}

export function SidebarDivider({ width, onResize }: SidebarDividerProps): JSX.Element {
  const origin = useRef<DragOrigin | null>(null)

  function onPointerDown(event: PointerEvent<HTMLDivElement>): void {
    // Without this the grab lands as a click on the row underneath, selecting a
    // pane the user was only resizing.
    event.preventDefault()

    origin.current = { pointerX: event.clientX, width }
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>): void {
    const start = origin.current
    if (!start) {
      return
    }

    const travelled = event.clientX - start.pointerX
    onResize(start.width + travelled)
  }

  function endDrag(event: PointerEvent<HTMLDivElement>): void {
    if (!origin.current) {
      return
    }

    origin.current = null
    event.currentTarget.releasePointerCapture(event.pointerId)
  }

  function onDoubleClick(event: MouseEvent<HTMLDivElement>): void {
    const sidebar = event.currentTarget.parentElement
    if (!sidebar) {
      return
    }

    const labels = sidebar.querySelectorAll<HTMLElement>(ROW_LABEL_SELECTOR)
    if (labels.length === 0) {
      return
    }

    const rowWidths: number[] = []
    labels.forEach((label) => {
      const row = label.closest<HTMLElement>(TERMINAL_ROW_SELECTOR)
      if (!row) {
        return
      }

      const rowWidth = row.getBoundingClientRect().width
      const labelWidth = label.getBoundingClientRect().width
      const spaceAroundLabel = rowWidth - labelWidth
      const textWidth = textWidthOf(label)
      rowWidths.push(Math.ceil(textWidth + spaceAroundLabel))
    })

    if (rowWidths.length === 0) {
      return
    }

    const widest = Math.max(...rowWidths)
    onResize(widest)
  }

  return (
    <div
      className="sidebar-divider"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onDoubleClick={onDoubleClick}
    />
  )
}
