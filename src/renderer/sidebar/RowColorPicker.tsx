import type { JSX } from 'react'
import { ROW_COLORS, type RowColor } from '@shared/model'
import { usePopup } from './usePopup'

interface RowColorPickerProps {
  x: number
  y: number
  current: RowColor | null
  canFork: boolean
  forkName: string
  onPreview: (color: RowColor | null) => void
  onPreviewEnd: () => void
  onPick: (color: RowColor | null) => void
  onFork: () => void
  onNewGroup: () => void
  onClose: () => void
}

export function rowColorVariable(color: RowColor): string {
  return `var(--row-color-${color})`
}

export function RowColorPicker({
  x,
  y,
  current,
  canFork,
  forkName,
  onPreview,
  onPreviewEnd,
  onPick,
  onFork,
  onNewGroup,
  onClose
}: RowColorPickerProps): JSX.Element {
  const { popup, position } = usePopup(x, y, onClose)

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
      ref={popup}
      className="row-color-picker"
      style={position}
      // Keeps keyboard focus in the terminal while the popup is clicked.
      onMouseDown={(event) => {
        event.preventDefault()
      }}
    >
      <div className="row-color-dots">
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
      <button type="button" className="row-fork-item" disabled={!canFork} onClick={onFork}>
        Fork with {forkName}
      </button>
      <button type="button" className="row-fork-item row-new-group-item" onClick={onNewGroup}>
        New group
      </button>
    </div>
  )
}
