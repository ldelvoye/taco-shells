import { type JSX, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ROW_COLORS, type RowColor } from '@shared/model'

const WINDOW_EDGE_GAP = 6

interface RowColorPickerProps {
  x: number
  y: number
  current: RowColor | null
  onPreview: (color: RowColor | null) => void
  onPreviewEnd: () => void
  onPick: (color: RowColor | null) => void
  onClose: () => void
}

export function rowColorVariable(color: RowColor): string {
  return `var(--row-color-${color})`
}

export function RowColorPicker({
  x,
  y,
  current,
  onPreview,
  onPreviewEnd,
  onPick,
  onClose
}: RowColorPickerProps): JSX.Element {
  const picker = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ left: x, top: y })

  useLayoutEffect(() => {
    const element = picker.current
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
    // Capture phase: Esc would otherwise reach the program in the terminal and interrupt Claude.
    function onKeyDown(event: KeyboardEvent): void {
      if (event.code !== 'Escape') {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      onClose()
    }

    function onMouseDown(event: MouseEvent): void {
      const element = picker.current
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

  const dots = ROW_COLORS.map((color) => {
    let className = 'row-color-dot'
    if (color === current) {
      className += ' is-current'
    }

    const background = rowColorVariable(color)
    return (
      <button
        key={color}
        type="button"
        className={className}
        style={{ background }}
        data-color={color}
        aria-label={color}
        onMouseEnter={() => {
          onPreview(color)
        }}
        onMouseLeave={onPreviewEnd}
        onClick={() => {
          onPick(color)
        }}
      />
    )
  })

  return (
    <div
      ref={picker}
      className="row-color-picker"
      style={position}
      // Keeps keyboard focus in the terminal while the dots are clicked.
      onMouseDown={(event) => {
        event.preventDefault()
      }}
    >
      {dots}
      <button
        type="button"
        className="row-color-dot is-none"
        data-color="none"
        aria-label="no color"
        onMouseEnter={() => {
          onPreview(null)
        }}
        onMouseLeave={onPreviewEnd}
        onClick={() => {
          onPick(null)
        }}
      />
    </div>
  )
}
