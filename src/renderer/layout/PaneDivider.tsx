import { type JSX, type PointerEvent, useRef } from 'react'

interface DragOrigin {
  pointerX: number
  leftWidth: number
  groupWidth: number
}

interface PaneDividerProps {
  leftWidth: number
  onResize: (leftWidth: number) => void
}

export function PaneDivider({ leftWidth, onResize }: PaneDividerProps): JSX.Element {
  const origin = useRef<DragOrigin | null>(null)

  function onPointerDown(event: PointerEvent<HTMLDivElement>): void {
    const group = event.currentTarget.parentElement
    if (!group) {
      return
    }

    const bounds = group.getBoundingClientRect()
    if (bounds.width === 0) {
      return
    }

    // Without this the grab lands as a click on the terminal underneath, moving
    // focus to a pane the user was only resizing.
    event.preventDefault()

    origin.current = { pointerX: event.clientX, leftWidth, groupWidth: bounds.width }
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>): void {
    const start = origin.current
    if (!start) {
      return
    }

    const travelled = event.clientX - start.pointerX
    const travelledShare = travelled / start.groupWidth
    onResize(start.leftWidth + travelledShare)
  }

  function endDrag(event: PointerEvent<HTMLDivElement>): void {
    if (!origin.current) {
      return
    }

    origin.current = null
    event.currentTarget.releasePointerCapture(event.pointerId)
  }

  return (
    <div
      className="pane-divider"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
    />
  )
}
