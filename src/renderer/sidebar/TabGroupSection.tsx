import type { CSSProperties, DragEvent, JSX, MouseEvent } from 'react'
import type { SessionId } from '@shared/ipc'
import type { DropPoint } from '@shared/drop'
import type { DropMark } from '@shared/dropMark'
import type { Group, Pane, RowColor, TabGroup, TabGroupId } from '@shared/model'
import { type AgentRows, summaryState } from '@shared/rowState'
import { PlusIcon } from './icons'
import { rowColorVariable } from './RowColorPicker'
import { SidebarAction } from './SidebarAction'
import type { SidebarActions } from './sidebarActions'
import { SidebarEntry } from './SidebarEntry'
import { edgeClass, RIGHT_BUTTON } from './sidebarShared'
import type { SidebarDrag } from './useSidebarDrag'

const UNNAMED_TAB_GROUP = 'Group'

interface TabGroupSectionProps {
  tabGroup: TabGroup
  groups: Group[]
  first: number
  last: number
  activeGroup: number
  agentRows: AgentRows
  shownColor: (pane: Pane) => RowColor | null
  drag: SidebarDrag
  actions: SidebarActions
  onOpenPicker: (event: MouseEvent<HTMLDivElement>, paneId: SessionId) => void
  onOpenPopup: (event: MouseEvent<HTMLDivElement>, tabGroup: TabGroupId) => void
}

function wrapperMarkClass(mark: DropMark | null, id: TabGroupId): string {
  if (mark === null) {
    return ''
  }
  const target = mark.target
  if (target.on !== 'wrapper' || target.tabGroup !== id) {
    return ''
  }
  return edgeClass(mark)
}

function headerMarkClass(mark: DropMark | null, id: TabGroupId): string {
  if (mark === null) {
    return ''
  }
  const target = mark.target
  if (target.on !== 'header' || target.tabGroup !== id) {
    return ''
  }
  return edgeClass(mark)
}

function tabGroupLabel(name: string): string {
  if (name.trim()) {
    return name
  }
  return UNNAMED_TAB_GROUP
}

function headerPointOf(event: DragEvent<HTMLDivElement>, tabGroup: TabGroupId): DropPoint {
  const bounds = event.currentTarget.getBoundingClientRect()
  const offsetY = event.clientY - bounds.top
  const raised = Math.max(offsetY / bounds.height, 0)
  const fraction = Math.min(raised, 1)
  return { over: 'header', tabGroup, fraction }
}

export function TabGroupSection({
  tabGroup,
  groups,
  first,
  last,
  activeGroup,
  agentRows,
  shownColor,
  drag,
  actions,
  onOpenPicker,
  onOpenPopup
}: TabGroupSectionProps): JSX.Element {
  const members = groups.slice(first, last + 1)
  let shownEntries: JSX.Element[] = []
  let className = 'tab-group'
  let headerClassName = 'tab-group-header'
  let count = null
  if (tabGroup.collapsed) {
    className += ' is-collapsed'
    const memberPanes = members.flatMap((member) => member.panes)
    const memberIds = memberPanes.map((pane) => pane.id)
    const summary = summaryState(agentRows, memberIds)
    let marker = null
    if (summary !== null) {
      marker = <span className="row-state" data-state={summary} />
    }
    count = (
      <>
        <span className="tab-group-count">{memberPanes.length}</span>
        {marker}
      </>
    )
    if (activeGroup >= first && activeGroup <= last) {
      headerClassName += ' is-active'
    }
  } else {
    shownEntries = members.map((member, offset) => {
      const groupIndex = first + offset
      return (
        <SidebarEntry
          key={member.id}
          group={member}
          groupIndex={groupIndex}
          activeGroup={activeGroup}
          agentRows={agentRows}
          shownColor={shownColor}
          drag={drag}
          actions={actions}
          onOpenPicker={onOpenPicker}
        />
      )
    })
  }

  className += wrapperMarkClass(drag.mark, tabGroup.id)
  headerClassName += headerMarkClass(drag.mark, tabGroup.id)

  const tabStyle = { '--tab-color': rowColorVariable(tabGroup.color) } as CSSProperties
  const label = tabGroupLabel(tabGroup.name)
  return (
    <div className={className} data-tab-group={tabGroup.id} style={tabStyle}>
      <div
        className={headerClassName}
        draggable
        onDragStart={(event) => {
          drag.start(event, { kind: 'tabGroup', tabGroup: tabGroup.id }, label)
          drag.showWhole(event, '.tab-group')
        }}
        onDragOver={(event) => {
          const point = headerPointOf(event, tabGroup.id)
          drag.over(event, point)
        }}
        onDrop={(event) => {
          const point = headerPointOf(event, tabGroup.id)
          drag.dropAt(event, point)
        }}
        onDragEnd={drag.end}
        onClick={() => {
          actions.toggleTabGroup(tabGroup.id)
        }}
        onMouseDown={(event) => {
          if (event.button === RIGHT_BUTTON) {
            event.preventDefault()
          }
        }}
        onContextMenu={(event) => {
          onOpenPopup(event, tabGroup.id)
        }}
      >
        <span className="tab-group-caret" />
        <span className="tab-group-name">{label}</span>
        {count}
        <SidebarAction
          className="tab-group-new"
          label={`New terminal in ${label}`}
          onActivate={() => {
            actions.openTerminalIn(tabGroup.id)
          }}
        >
          <PlusIcon />
        </SidebarAction>
      </div>
      {shownEntries}
    </div>
  )
}
