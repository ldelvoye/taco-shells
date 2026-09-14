import type { JSX } from 'react'
import type { Group } from '@shared/model'
import { TerminalView } from '../terminal/TerminalView'
import { PaneDivider } from './PaneDivider'

interface PaneGroupProps {
  group: Group
  active: boolean
  onResize: (dividerIndex: number, leftWidth: number) => void
}

function PaneGroup({ group, active, onResize }: PaneGroupProps): JSX.Element {
  const lastIndex = group.panes.length - 1
  const contents: JSX.Element[] = []

  group.panes.forEach((pane, paneIndex) => {
    contents.push(<TerminalView key={pane.id} sessionId={pane.id} width={pane.width} />)

    if (paneIndex < lastIndex) {
      contents.push(
        <PaneDivider
          key={`divider-after-${pane.id}`}
          leftWidth={pane.width}
          onResize={(leftWidth) => {
            onResize(paneIndex, leftWidth)
          }}
        />
      )
    }
  })

  let className = 'pane-group'
  if (!active) {
    className += ' is-inactive'
  }

  return <div className={className}>{contents}</div>
}

interface PaneAreaProps {
  groups: Group[]
  activeGroup: number
  onResize: (groupIndex: number, dividerIndex: number, leftWidth: number) => void
}

export function PaneArea({ groups, activeGroup, onResize }: PaneAreaProps): JSX.Element {
  const rendered = groups.map((group, groupIndex) => {
    return (
      <PaneGroup
        key={group.id}
        group={group}
        active={groupIndex === activeGroup}
        onResize={(dividerIndex, leftWidth) => {
          onResize(groupIndex, dividerIndex, leftWidth)
        }}
      />
    )
  })

  return <main className="pane-area">{rendered}</main>
}
