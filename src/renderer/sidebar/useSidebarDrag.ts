import { type DragEvent, useRef, useState } from 'react'
import { type DropPoint, dropFor } from '@shared/drop'
import { type DropMark, dropMarkFor, sameDrop } from '@shared/dropMark'
import type { Dragged, Drop, Workspace } from '@shared/model'

export interface SidebarDrag {
  drop: Drop | null
  mark: DropMark | null
  start(event: DragEvent<HTMLElement>, what: Dragged, label: string): void
  showWhole(event: DragEvent<HTMLElement>, selector: string): void
  over(event: DragEvent<HTMLDivElement>, point: DropPoint): void
  dropAt(event: DragEvent<HTMLDivElement>, point: DropPoint): void
  end(): void
  leaveRows(event: DragEvent<HTMLDivElement>): void
}

export function useSidebarDrag(
  workspace: Workspace,
  onDrop: (dragged: Dragged, drop: Drop) => void
): SidebarDrag {
  const dragged = useRef<Dragged | null>(null)
  const [drop, setDrop] = useState<Drop | null>(null)

  function start(event: DragEvent<HTMLElement>, what: Dragged, label: string): void {
    dragged.current = what
    event.dataTransfer.effectAllowed = 'move'
    // The drop reads the ref above, not this. The label is what a drag that lands outside the sidebar drops.
    event.dataTransfer.setData('text/plain', label)
  }

  // The bracket is what was grabbed, but the picture that follows the pointer is the whole split.
  function showWhole(event: DragEvent<HTMLElement>, selector: string): void {
    const shown = event.currentTarget.closest(selector)
    if (shown === null) {
      return
    }

    const bounds = shown.getBoundingClientRect()
    const offsetX = event.clientX - bounds.left
    const offsetY = event.clientY - bounds.top
    event.dataTransfer.setDragImage(shown, offsetX, offsetY)
  }

  function landingAt(point: DropPoint): Drop | null {
    const dragging = dragged.current
    if (dragging === null) {
      return null
    }
    return dropFor(workspace, dragging, point)
  }

  function over(event: DragEvent<HTMLDivElement>, point: DropPoint): void {
    const next = landingAt(point)
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

  function dropAt(event: DragEvent<HTMLDivElement>, point: DropPoint): void {
    event.preventDefault()

    const landed = landingAt(point)
    const draggedValue = dragged.current
    end()

    if (draggedValue === null || landed === null) {
      return
    }
    onDrop(draggedValue, landed)
  }

  function end(): void {
    dragged.current = null
    setDrop(null)
  }

  // A dragleave also fires when the pointer moves between two rows, so only one that ends outside the rows counts.
  function leaveRows(event: DragEvent<HTMLDivElement>): void {
    const bounds = event.currentTarget.getBoundingClientRect()
    const insideAcross = event.clientX >= bounds.left && event.clientX < bounds.right
    const insideDown = event.clientY >= bounds.top && event.clientY < bounds.bottom
    if (insideAcross && insideDown) {
      return
    }
    setDrop(null)
  }

  const mark = dropMarkFor(workspace, drop)

  return { drop, mark, start, showWhole, over, dropAt, end, leaveRows }
}
