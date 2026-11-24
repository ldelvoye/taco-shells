import type { JSX } from 'react'
import { ROW_COLORS, type RowColor } from '@shared/model'
import { rowColorVariable } from './rowColor'
import { usePopup } from './usePopup'

interface RowPopupProps {
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

export function RowPopup({
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
}: RowPopupProps): JSX.Element {
  const { popup, position } = usePopup(x, y, onClose)

  const dots = ROW_COLORS.map((color) => {
    let className = 'color-dot'
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
      className="popup"
      style={position}
      // Keeps keyboard focus in the terminal while the popup is clicked.
      onMouseDown={(event) => {
        event.preventDefault()
      }}
    >
      <div className="popup-dots">
        {dots}
        <button
          type="button"
          className="color-dot is-none"
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
      <button type="button" className="popup-item" disabled={!canFork} onClick={onFork}>
        Fork with {forkName}
      </button>
      <button type="button" className="popup-item row-new-group-item" onClick={onNewGroup}>
        New group
      </button>
    </div>
  )
}
