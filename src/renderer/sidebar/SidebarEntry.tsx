import type { DragEvent, JSX, MouseEvent } from 'react'
import type { SessionId } from '@shared/ipc'
import { type DropPoint, rowPointIn } from '@shared/drop'
import type { DropMark } from '@shared/dropMark'
import type { Group, Pane, RowColor } from '@shared/model'
import type { AgentRows, RowState } from '@shared/rowState'
import type { SidebarActions } from './sidebarActions'
import { edgeClass } from './sidebarShared'
import { TerminalRow } from './TerminalRow'
import type { SidebarDrag } from './useSidebarDrag'

interface SidebarEntryProps {
  group: Group
  groupIndex: number
  activeGroup: number
  agentRows: AgentRows
  shownColor: (pane: Pane) => RowColor | null
  drag: SidebarDrag
  actions: SidebarActions
  onOpenPicker: (event: MouseEvent<HTMLDivElement>, paneId: SessionId) => void
}

function entryMarkClass(mark: DropMark | null, index: number): string {
  if (mark === null) {
    return ''
  }
  const target = mark.target
  if (target.on !== 'entry' || target.index !== index) {
    return ''
  }
  return edgeClass(mark)
}

export function SidebarEntry({
  group,
  groupIndex,
  activeGroup,
  agentRows,
  shownColor,
  drag,
  actions,
  onOpenPicker
}: SidebarEntryProps): JSX.Element {
  function rowPointOf(event: DragEvent<HTMLDivElement>): DropPoint {
    const paneCount = group.panes.length
    const bounds = event.currentTarget.getBoundingClientRect()
    const offsetY = event.clientY - bounds.top
    const point = rowPointIn(groupIndex, paneCount, offsetY, bounds.height)
    return { over: 'row', ...point }
  }

  const drop = drag.drop
  let joinSlot: number | null = null
  if (drop !== null && drop.kind === 'into' && drop.group === group.id) {
    joinSlot = drop.slot
  }

  const rows = group.panes.map((pane, paneIndex) => {
    let markerState: RowState | undefined
    const agentRow = agentRows.get(pane.id)
    if (agentRow !== undefined) {
      markerState = agentRow.state
    }

    const isActivePane = groupIndex === activeGroup && paneIndex === group.activePane
    const color = shownColor(pane)
    return (
      <TerminalRow
        key={pane.id}
        group={group}
        pane={pane}
        paneIndex={paneIndex}
        active={isActivePane}
        joinSlot={joinSlot}
        color={color}
        markerState={markerState}
        drag={drag}
        actions={actions}
        onOpenPicker={onOpenPicker}
      />
    )
  })

  let className = 'terminal-group'
  className += entryMarkClass(drag.mark, groupIndex)
  if (joinSlot !== null) {
    className += ' is-join-target'
  }

  // A row drags its one terminal and the bracket drags the whole split, so the group only receives drops.
  return (
    <div
      className={className}
      onDragOver={(event) => {
        const point = rowPointOf(event)
        drag.over(event, point)
      }}
      onDrop={(event) => {
        const point = rowPointOf(event)
        drag.dropAt(event, point)
      }}
      onDragEnd={drag.end}
    >
      {rows}
    </div>
  )
}
