import { type JSX, type KeyboardEvent, useEffect, useRef } from 'react'
import { ROW_COLORS, type RowColor } from '@shared/model'
import { rowColorVariable } from './RowColorPicker'
import { usePopup } from './usePopup'

interface TabGroupPopupProps {
  x: number
  y: number
  name: string
  color: RowColor
  onRename: (name: string) => void
  onRecolor: (color: RowColor) => void
  onUngroup: () => void
  onClose: () => void
}

function handleClipboardKey(
  event: KeyboardEvent<HTMLInputElement>,
  onRename: (name: string) => void
): void {
  if (!event.metaKey) {
    return
  }

  const field = event.currentTarget
  const key = event.key.toLowerCase()
  if (key === 'a') {
    event.preventDefault()
    field.select()
    return
  }

  if (key === 'c' || key === 'x') {
    event.preventDefault()
    const start = field.selectionStart
    const end = field.selectionEnd
    if (start === null || end === null) {
      return
    }

    const selected = field.value.slice(start, end)
    window.tacoShells.clipboard.write(selected)
    if (key === 'c') {
      return
    }

    const before = field.value.slice(0, start)
    const after = field.value.slice(end)
    const remaining = before + after
    field.value = remaining
    field.setSelectionRange(start, start)
    onRename(remaining)
    return
  }

  if (key === 'v') {
    event.preventDefault()
    void window.tacoShells.clipboard.read().then((text) => {
      const start = field.selectionStart
      const end = field.selectionEnd
      if (start === null || end === null) {
        return
      }

      const before = field.value.slice(0, start)
      const after = field.value.slice(end)
      const pasted = before + text + after
      const caret = start + text.length
      field.value = pasted
      field.setSelectionRange(caret, caret)
      onRename(pasted)
    })
  }
}

export function TabGroupPopup({
  x,
  y,
  name,
  color,
  onRename,
  onRecolor,
  onUngroup,
  onClose
}: TabGroupPopupProps): JSX.Element {
  const { popup, position } = usePopup(x, y, onClose)
  const field = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const element = field.current
    if (!element) {
      return
    }
    element.focus()
    element.select()
  }, [])

  const dots = ROW_COLORS.map((dotColor) => {
    let className = 'row-color-dot'
    if (dotColor === color) {
      className += ' is-current'
    }

    const background = rowColorVariable(dotColor)
    return (
      <button
        key={dotColor}
        type="button"
        className={className}
        style={{ background }}
        data-color={dotColor}
        aria-label={dotColor}
        onMouseDown={(event) => {
          event.preventDefault()
        }}
        onClick={() => {
          onRecolor(dotColor)
        }}
      />
    )
  })

  return (
    <div ref={popup} className="row-color-picker tab-group-popup" style={position}>
      <input
        ref={field}
        className="tab-group-name-field"
        type="text"
        aria-label="Group name"
        value={name}
        onChange={(event) => {
          onRename(event.target.value)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault()
            onClose()
            return
          }
          handleClipboardKey(event, onRename)
        }}
      />
      <div className="row-color-dots">{dots}</div>
      <button
        type="button"
        className="row-fork-item tab-group-ungroup"
        onMouseDown={(event) => {
          event.preventDefault()
        }}
        onClick={() => {
          onUngroup()
          onClose()
        }}
      >
        Ungroup
      </button>
    </div>
  )
}
