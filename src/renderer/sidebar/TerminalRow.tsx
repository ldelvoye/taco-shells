import type { JSX, MouseEvent } from 'react'
import type { SessionId } from '@shared/ipc'
import { type Group, type Pane, paneOf, type RowColor } from '@shared/model'
import type { RowState } from '@shared/rowState'
import { TrashIcon } from './icons'
import { rowColorVariable } from './rowColor'
import { SidebarAction } from './SidebarAction'
import type { SidebarActions } from './sidebarActions'
import { RIGHT_BUTTON } from './sidebarShared'
import type { SidebarDrag } from './useSidebarDrag'

const UNTITLED_ROW = 'Terminal'

const CONNECTOR_FIRST = 'is-first'
const CONNECTOR_MIDDLE = 'is-middle'
const CONNECTOR_LAST = 'is-last'

interface TerminalRowProps {
  group: Group
  pane: Pane
  paneIndex: number
  active: boolean
  joinSlot: number | null
  color: RowColor | null
  markerState: RowState | undefined
  drag: SidebarDrag
  actions: SidebarActions
  onOpenRowPopup: (event: MouseEvent<HTMLDivElement>, paneId: SessionId) => void
}

function labelFor(title: string): string {
  if (title) {
    return title
  }
  return UNTITLED_ROW
}

// CSS, not box-drawing glyphs: their em box is shorter than a row, so the
// strokes of adjacent rows would not quite meet.
function connectorFor(paneIndex: number, paneCount: number): string | null {
  if (paneCount < 2) {
    return null
  }
  if (paneIndex === 0) {
    return CONNECTOR_FIRST
  }
  if (paneIndex === paneCount - 1) {
    return CONNECTOR_LAST
  }
  return CONNECTOR_MIDDLE
}

export function TerminalRow({
  group,
  pane,
  paneIndex,
  active,
  joinSlot,
  color,
  markerState,
  drag,
  actions,
  onOpenRowPopup
}: TerminalRowProps): JSX.Element {
  const paneCount = group.panes.length
  const label = labelFor(pane.title)

  let connector = null
  const connectorPiece = connectorFor(paneIndex, paneCount)
  if (connectorPiece) {
    const activeInGroup = paneOf(group)
    const groupDragLabel = labelFor(activeInGroup.title)
    connector = (
      <span
        className={`row-connector ${connectorPiece}`}
        draggable
        onDragStart={(event) => {
          event.stopPropagation()
          drag.start(event, { kind: 'group', group: group.id }, groupDragLabel)
          drag.showWhole(event, '.terminal-group')
        }}
      />
    )
  }

  let className = 'terminal-row'
  if (active) {
    className += ' is-active'
  }
  if (joinSlot === paneIndex) {
    className += ' drop-before'
  }
  if (joinSlot === paneCount && paneIndex === paneCount - 1) {
    className += ' drop-after'
  }

  let stripe = null
  if (color !== null) {
    const background = rowColorVariable(color)
    stripe = <span className="row-color" data-color={color} style={{ background }} />
  }

  return (
    <div
      className={className}
      title={label}
      draggable
      onDragStart={(event) => {
        drag.start(event, { kind: 'pane', pane: pane.id }, label)
      }}
      onClick={() => {
        actions.select(pane.id)
      }}
      onMouseDown={(event) => {
        // Keeps keyboard focus in the terminal: right-clicking a row only opens the popup.
        if (event.button === RIGHT_BUTTON) {
          event.preventDefault()
        }
      }}
      onContextMenu={(event) => {
        onOpenRowPopup(event, pane.id)
      }}
    >
      {stripe}
      {connector}
      <span className="row-state" data-state={markerState} />
      <span className="row-label">{label}</span>
      <SidebarAction
        className="row-close"
        label="Close terminal"
        onActivate={() => {
          actions.close(pane.id)
        }}
      >
        <TrashIcon />
      </SidebarAction>
    </div>
  )
}
