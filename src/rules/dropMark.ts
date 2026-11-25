import type { Drop, TabGroupId, Workspace } from '@protocol/arrangement'

import { blockOf } from './arrangement'

export type MarkTarget =
  | { on: 'entry'; index: number }
  | { on: 'wrapper'; tabGroup: TabGroupId }
  | { on: 'header'; tabGroup: TabGroupId }

export interface DropMark {
  target: MarkTarget
  edge: 'above' | 'below' | 'whole'
  tabGroup: TabGroupId | null
}

export function sameDrop(a: Drop | null, b: Drop | null): boolean {
  if (a === null || b === null) {
    return a === b
  }
  if (a.kind === 'between' && b.kind === 'between') {
    return a.index === b.index && a.tabGroup === b.tabGroup
  }
  if (a.kind === 'into' && b.kind === 'into') {
    return a.group === b.group && a.slot === b.slot
  }
  return false
}

function markInNoGroup(workspace: Workspace, index: number): DropMark | null {
  const groups = workspace.groups
  if (index === groups.length) {
    const lastIndex = groups.length - 1
    const last = groups[lastIndex]
    if (last === undefined) {
      return null
    }
    if (last.tabGroup === null) {
      return { target: { on: 'entry', index: lastIndex }, edge: 'below', tabGroup: null }
    }
    return { target: { on: 'wrapper', tabGroup: last.tabGroup }, edge: 'below', tabGroup: null }
  }

  const entry = groups[index]
  if (entry.tabGroup !== null) {
    const block = blockOf(workspace, entry.tabGroup)
    if (block !== null && block.first === index) {
      return { target: { on: 'wrapper', tabGroup: entry.tabGroup }, edge: 'above', tabGroup: null }
    }
  }
  return { target: { on: 'entry', index }, edge: 'above', tabGroup: null }
}

function markInTabGroup(workspace: Workspace, id: TabGroupId, index: number): DropMark | null {
  const record = workspace.tabGroups.find((candidate) => candidate.id === id)
  const block = blockOf(workspace, id)
  if (record === undefined || block === null) {
    return null
  }

  if (record.collapsed) {
    return { target: { on: 'header', tabGroup: id }, edge: 'whole', tabGroup: id }
  }
  if (index === block.first) {
    return { target: { on: 'header', tabGroup: id }, edge: 'below', tabGroup: id }
  }
  if (index > block.last) {
    return { target: { on: 'entry', index: block.last }, edge: 'below', tabGroup: id }
  }
  return { target: { on: 'entry', index }, edge: 'above', tabGroup: id }
}

export function dropMarkFor(workspace: Workspace, drop: Drop | null): DropMark | null {
  if (drop === null || drop.kind !== 'between') {
    return null
  }
  if (drop.tabGroup === null) {
    return markInNoGroup(workspace, drop.index)
  }
  return markInTabGroup(workspace, drop.tabGroup, drop.index)
}
