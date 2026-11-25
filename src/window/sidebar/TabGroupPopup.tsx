import { type JSX, type KeyboardEvent, useEffect, useRef, useState } from 'react'
import { ROW_COLORS, type RowColor } from '@protocol/arrangement'
import { rowColorVariable } from './rowColor'
import { usePopup } from './usePopup'

const LINE_BREAKS = /[\r\n]+/g

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

// macOS gives a text field these keys through the Edit menu, and the app has none.
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
    void window.tacoShells.clipboard.read().then((clipboard) => {
      // The popup can close while the clipboard is being read.
      if (!field.isConnected) {
        return
      }
      const start = field.selectionStart
      const end = field.selectionEnd
      if (start === null || end === null) {
        return
      }

      const text = clipboard.replace(LINE_BREAKS, ' ')
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
  const [text, setText] = useState(name)

  function rename(next: string): void {
    setText(next)
    onRename(next)
  }

  useEffect(() => {
    const element = field.current
    if (!element) {
      return
    }
    element.focus()
    element.select()
  }, [])

  const dots = ROW_COLORS.map((dotColor) => {
    let className = 'color-dot'
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
    <div ref={popup} className="popup tab-group-popup" style={position}>
      <input
        ref={field}
        className="tab-group-name-field"
        type="text"
        aria-label="Group name"
        value={text}
        onChange={(event) => {
          rename(event.target.value)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault()
            onClose()
            return
          }
          handleClipboardKey(event, rename)
        }}
      />
      <div className="popup-dots">{dots}</div>
      <button
        type="button"
        className="popup-item tab-group-ungroup"
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
