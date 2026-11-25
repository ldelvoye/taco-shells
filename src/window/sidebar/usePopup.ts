import { type RefObject, useEffect, useLayoutEffect, useRef, useState } from 'react'

const WINDOW_EDGE_GAP = 6

interface PopupPosition {
  left: number
  top: number
}

interface PopupHandle {
  popup: RefObject<HTMLDivElement | null>
  position: PopupPosition
}

export function usePopup(x: number, y: number, onClose: () => void): PopupHandle {
  const popup = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ left: x, top: y })

  useLayoutEffect(() => {
    const element = popup.current
    if (!element) {
      return
    }

    const bounds = element.getBoundingClientRect()
    const furthestLeft = window.innerWidth - bounds.width - WINDOW_EDGE_GAP
    const furthestTop = window.innerHeight - bounds.height - WINDOW_EDGE_GAP
    const left = Math.min(x, furthestLeft)
    const top = Math.min(y, furthestTop)
    setPosition({ left, top })
  }, [x, y])

  useEffect(() => {
    // Capture phase: Esc would otherwise reach the program in the terminal and interrupt an agent.
    function onKeyDown(event: KeyboardEvent): void {
      if (event.code !== 'Escape') {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      onClose()
    }

    function onMouseDown(event: MouseEvent): void {
      const element = popup.current
      if (element && element.contains(event.target as Node)) {
        return
      }
      onClose()
    }

    window.addEventListener('keydown', onKeyDown, true)
    window.addEventListener('mousedown', onMouseDown, true)
    // A click in another app reaches this window only as a blur.
    window.addEventListener('blur', onClose)
    return () => {
      window.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('mousedown', onMouseDown, true)
      window.removeEventListener('blur', onClose)
    }
  }, [onClose])

  return { popup, position }
}
