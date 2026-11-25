import { describe, expect, it } from 'vitest'

import type { Dragged, Drop, Group, RowColor, Workspace } from '@protocol/arrangement'
import { ROW_COLORS } from '@protocol/arrangement'

import { MIN_PANE_WIDTH, emptyWorkspace, isHidden } from '../arrangement'
import { dropFor } from '../drop'
import type { DropPoint } from '../drop'
import { applyDrop } from './applyDrop'
import {
  activatePane,
  addGroup,
  addGroupTo,
  closePane,
  focusNextGroup,
  focusNextPane,
  focusPreviousGroup,
  focusPreviousPane,
  forkPane,
  moveDivider,
  renamePane,
  setPaneColor,
  splitPane,
  toggleActivePaneColor
} from './arrangement'
import {
  createTabGroup,
  recolorTabGroup,
  renameTabGroup,
  toggleTabGroup,
  ungroupTabGroup
} from './tabGroups'

const SEEDS = Array.from({ length: 40 }, (_, index) => index + 1)

const STEPS_PER_SEED = 500

const WIDTH_TOLERANCE = 1e-9

const MAX_PANES_PER_GROUP = 10

const MISSING_ID_CHANCE = 0.15

const NO_TAB_GROUP_CHANCE = 0.3

const PREFERRED_CHANCE = 0.8

const TITLES = ['', 'one', 'two']

const NEW_NAMES = ['', 'alpha', 'beta']

const MIN_ENTRY_CAP = 3

const ENTRY_CAP_SPREAD = 5

const GROWING_BUILDERS: Builder[] = [
  addGroupStep,
  addGroupToStep,
  applyDropFromPointerStep,
  applyRawDropStep
]

const CHECKS: Check[] = [
  tabGroupEntriesAreAdjacent,
  tabGroupsMatchEntries,
  focusIsValidAndShowing,
  everyGroupHasValidPanes,
  idsAreUnique
]

type Random = () => number

interface Counters {
  pane: number
  group: number
  tabGroup: number
}

interface FuzzState {
  random: Random
  counters: Counters
  workspace: Workspace
}

interface Step {
  label: string
  apply: (workspace: Workspace) => Workspace
}

type Builder = (state: FuzzState) => Step

type Check = (workspace: Workspace) => string | null

function seededRandom(seed: number): Random {
  let state = seed
  return () => {
    state = (state + 0x6d2b79f5) | 0
    let mixed = Math.imul(state ^ (state >>> 15), 1 | state)
    mixed = (mixed + Math.imul(mixed ^ (mixed >>> 7), 61 | mixed)) ^ mixed
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296
  }
}

function below(random: Random, bound: number): number {
  const scaled = random() * bound
  return Math.floor(scaled)
}

function chance(random: Random, probability: number): boolean {
  return random() < probability
}

function pickFrom<T>(random: Random, items: T[]): T {
  const index = below(random, items.length)
  return items[index]
}

function newId(state: FuzzState, kind: keyof Counters): string {
  state.counters[kind] += 1
  return `${kind}-${state.counters[kind]}`
}

function paneIdsOf(workspace: Workspace): string[] {
  return workspace.groups.flatMap((group) => group.panes.map((pane) => pane.id))
}

function groupIdsOf(workspace: Workspace): string[] {
  return workspace.groups.map((group) => group.id)
}

function tabGroupIdsOf(workspace: Workspace): string[] {
  return workspace.tabGroups.map((tabGroup) => tabGroup.id)
}

function existingOrMissing(state: FuzzState, ids: string[]): string {
  if (ids.length === 0 || chance(state.random, MISSING_ID_CHANCE)) {
    const number = below(state.random, 3)
    return `missing-${number}`
  }
  return pickFrom(state.random, ids)
}

function tabGroupOrNull(state: FuzzState): string | null {
  if (chance(state.random, NO_TAB_GROUP_CHANCE)) {
    return null
  }
  return existingOrMissing(state, tabGroupIdsOf(state.workspace))
}

function indexAround(state: FuzzState, length: number): number {
  const spread = below(state.random, length + 3)
  return spread - 1
}

function freshGroup(state: FuzzState): Group {
  const id = newId(state, 'group')
  const paneId = newId(state, 'pane')
  return {
    id,
    panes: [{ id: paneId, title: '', width: 1, color: null }],
    activePane: 0,
    tabGroup: null
  }
}

function randomColor(state: FuzzState): RowColor {
  return pickFrom(state.random, [...ROW_COLORS])
}

function randomColorOrNull(state: FuzzState): RowColor | null {
  if (chance(state.random, NO_TAB_GROUP_CHANCE)) {
    return null
  }
  return randomColor(state)
}

function randomDragged(state: FuzzState): Dragged {
  const { random, workspace } = state
  const kind = pickFrom(random, ['pane', 'group', 'tabGroup'])
  if (kind === 'pane') {
    return { kind, pane: existingOrMissing(state, paneIdsOf(workspace)) }
  }
  if (kind === 'group') {
    return { kind, group: existingOrMissing(state, groupIdsOf(workspace)) }
  }
  return { kind: 'tabGroup', tabGroup: existingOrMissing(state, tabGroupIdsOf(workspace)) }
}

function randomDropPoint(state: FuzzState): DropPoint {
  const { random, workspace } = state
  const fraction = random()
  const over = pickFrom(random, ['row', 'header', 'end'])
  if (over === 'row') {
    const group = indexAround(state, workspace.groups.length)
    const pane = below(random, 5)
    return { over, group, pane, fraction }
  }
  if (over === 'header') {
    const tabGroup = existingOrMissing(state, tabGroupIdsOf(workspace))
    return { over, tabGroup, fraction }
  }
  return { over: 'end' }
}

function randomRawDrop(state: FuzzState): Drop {
  const { random, workspace } = state
  if (chance(random, 0.5)) {
    const index = indexAround(state, workspace.groups.length)
    return { kind: 'between', index, tabGroup: tabGroupOrNull(state) }
  }

  const group = existingOrMissing(state, groupIdsOf(workspace))
  const slot = indexAround(state, MAX_PANES_PER_GROUP)
  return { kind: 'into', group, slot }
}

function addGroupStep(state: FuzzState): Step {
  const group = freshGroup(state)
  const label = `addGroup(${group.id})`
  return { label, apply: (workspace) => addGroup(workspace, group) }
}

function addGroupToStep(state: FuzzState): Step {
  const group = freshGroup(state)
  const tabGroup = tabGroupOrNull(state)
  const label = `addGroupTo(${group.id}, ${tabGroup})`
  return { label, apply: (workspace) => addGroupTo(workspace, group, tabGroup) }
}

function closePaneStep(state: FuzzState): Step {
  const id = existingOrMissing(state, paneIdsOf(state.workspace))
  return { label: `closePane(${id})`, apply: (workspace) => closePane(workspace, id) }
}

function crowdedPaneIds(workspace: Workspace): string[] {
  const mostPanes = Math.max(...workspace.groups.map((group) => group.panes.length))
  const crowded = workspace.groups.filter((group) => group.panes.length === mostPanes)
  return crowded.flatMap((group) => group.panes.map((pane) => pane.id))
}

function splitPaneStep(state: FuzzState): Step {
  let candidates = paneIdsOf(state.workspace)
  if (candidates.length > 0 && chance(state.random, PREFERRED_CHANCE)) {
    candidates = crowdedPaneIds(state.workspace)
  }
  const donor = existingOrMissing(state, candidates)
  const id = newId(state, 'pane')
  const label = `splitPane(${donor}, ${id})`
  return { label, apply: (workspace) => splitPane(workspace, donor, id) }
}

function forkPaneStep(state: FuzzState): Step {
  const source = existingOrMissing(state, paneIdsOf(state.workspace))
  const id = newId(state, 'pane')
  const label = `forkPane(${source}, ${id})`
  return { label, apply: (workspace) => forkPane(workspace, source, id) }
}

function activatePaneStep(state: FuzzState): Step {
  const id = existingOrMissing(state, paneIdsOf(state.workspace))
  return { label: `activatePane(${id})`, apply: (workspace) => activatePane(workspace, id) }
}

function focusNextPaneStep(): Step {
  return { label: 'focusNextPane()', apply: focusNextPane }
}

function focusPreviousPaneStep(): Step {
  return { label: 'focusPreviousPane()', apply: focusPreviousPane }
}

function focusNextGroupStep(): Step {
  return { label: 'focusNextGroup()', apply: focusNextGroup }
}

function focusPreviousGroupStep(): Step {
  return { label: 'focusPreviousGroup()', apply: focusPreviousGroup }
}

function moveDividerStep(state: FuzzState): Step {
  const groupIndex = indexAround(state, state.workspace.groups.length)
  const dividerIndex = indexAround(state, MAX_PANES_PER_GROUP)
  const widthSpan = state.random() * 1.4
  const leftWidth = widthSpan - 0.2
  const label = `moveDivider(${groupIndex}, ${dividerIndex}, ${leftWidth})`
  return {
    label,
    apply: (workspace) => moveDivider(workspace, groupIndex, dividerIndex, leftWidth)
  }
}

function renamePaneStep(state: FuzzState): Step {
  const id = existingOrMissing(state, paneIdsOf(state.workspace))
  const title = pickFrom(state.random, TITLES)
  const label = `renamePane(${id}, '${title}')`
  return { label, apply: (workspace) => renamePane(workspace, id, title) }
}

function setPaneColorStep(state: FuzzState): Step {
  const id = existingOrMissing(state, paneIdsOf(state.workspace))
  const color = randomColorOrNull(state)
  const label = `setPaneColor(${id}, ${color})`
  return { label, apply: (workspace) => setPaneColor(workspace, id, color) }
}

function toggleActivePaneColorStep(state: FuzzState): Step {
  const color = randomColor(state)
  const label = `toggleActivePaneColor(${color})`
  return { label, apply: (workspace) => toggleActivePaneColor(workspace, color) }
}

function applyDropFromPointerStep(state: FuzzState): Step {
  const dragged = randomDragged(state)
  const point = randomDropPoint(state)
  const newGroup = newId(state, 'group')
  const label = `applyDrop(${JSON.stringify(dragged)}, dropFor(${JSON.stringify(point)}), ${newGroup})`
  return {
    label,
    apply: (workspace) => {
      const drop = dropFor(workspace, dragged, point)
      if (drop === null) {
        return workspace
      }
      return applyDrop(workspace, dragged, drop, newGroup)
    }
  }
}

function applyRawDropStep(state: FuzzState): Step {
  const dragged = randomDragged(state)
  const drop = randomRawDrop(state)
  const newGroup = newId(state, 'group')
  const label = `applyDrop(${JSON.stringify(dragged)}, ${JSON.stringify(drop)}, ${newGroup})`
  return { label, apply: (workspace) => applyDrop(workspace, dragged, drop, newGroup) }
}

function createTabGroupStep(state: FuzzState): Step {
  const ungrouped = state.workspace.groups.filter((group) => group.tabGroup === null)
  let candidates = groupIdsOf(state.workspace)
  if (ungrouped.length > 0 && chance(state.random, PREFERRED_CHANCE)) {
    candidates = ungrouped.map((group) => group.id)
  }
  const entry = existingOrMissing(state, candidates)
  const id = newId(state, 'tabGroup')
  const label = `createTabGroup(${entry}, ${id})`
  return { label, apply: (workspace) => createTabGroup(workspace, entry, id) }
}

function renameTabGroupStep(state: FuzzState): Step {
  const id = existingOrMissing(state, tabGroupIdsOf(state.workspace))
  const name = pickFrom(state.random, NEW_NAMES)
  const label = `renameTabGroup(${id}, '${name}')`
  return { label, apply: (workspace) => renameTabGroup(workspace, id, name) }
}

function recolorTabGroupStep(state: FuzzState): Step {
  const id = existingOrMissing(state, tabGroupIdsOf(state.workspace))
  const color = randomColor(state)
  const label = `recolorTabGroup(${id}, ${color})`
  return { label, apply: (workspace) => recolorTabGroup(workspace, id, color) }
}

function ungroupTabGroupStep(state: FuzzState): Step {
  const id = existingOrMissing(state, tabGroupIdsOf(state.workspace))
  return { label: `ungroupTabGroup(${id})`, apply: (workspace) => ungroupTabGroup(workspace, id) }
}

function toggleTabGroupStep(state: FuzzState): Step {
  const open = state.workspace.tabGroups.filter((tabGroup) => !tabGroup.collapsed)
  let candidates = tabGroupIdsOf(state.workspace)
  if (open.length > 0 && chance(state.random, PREFERRED_CHANCE)) {
    candidates = open.map((tabGroup) => tabGroup.id)
  }
  const id = existingOrMissing(state, candidates)
  return { label: `toggleTabGroup(${id})`, apply: (workspace) => toggleTabGroup(workspace, id) }
}

function times(count: number, builder: Builder): Builder[] {
  return Array.from({ length: count }, () => builder)
}

function weightedBuilders(): Builder[] {
  return [
    ...times(3, addGroupStep),
    ...times(2, addGroupToStep),
    ...times(3, closePaneStep),
    ...times(5, splitPaneStep),
    forkPaneStep,
    ...times(2, activatePaneStep),
    focusNextPaneStep,
    focusPreviousPaneStep,
    focusNextGroupStep,
    focusPreviousGroupStep,
    ...times(3, moveDividerStep),
    renamePaneStep,
    setPaneColorStep,
    toggleActivePaneColorStep,
    ...times(5, applyDropFromPointerStep),
    ...times(4, applyRawDropStep),
    ...times(6, createTabGroupStep),
    renameTabGroupStep,
    recolorTabGroupStep,
    ...times(5, ungroupTabGroupStep),
    ...times(8, toggleTabGroupStep)
  ]
}

function findDuplicate(ids: string[]): string | null {
  const seen = new Set<string>()
  for (const id of ids) {
    if (seen.has(id)) {
      return id
    }
    seen.add(id)
  }
  return null
}

function tabGroupEntriesAreAdjacent(workspace: Workspace): string | null {
  const finishedRuns = new Set<string>()
  let currentRun: string | null = null
  for (const group of workspace.groups) {
    if (group.tabGroup !== currentRun) {
      if (currentRun !== null) {
        finishedRuns.add(currentRun)
      }
      currentRun = group.tabGroup
    }
    if (group.tabGroup !== null && finishedRuns.has(group.tabGroup)) {
      return `tab group ${group.tabGroup} has entries in two separate runs`
    }
  }
  return null
}

function tabGroupsMatchEntries(workspace: Workspace): string | null {
  const duplicate = findDuplicate(tabGroupIdsOf(workspace))
  if (duplicate !== null) {
    return `tab group id ${duplicate} appears twice`
  }

  const usedIds = new Set(workspace.groups.map((group) => group.tabGroup))
  for (const tabGroup of workspace.tabGroups) {
    if (!usedIds.has(tabGroup.id)) {
      return `tab group ${tabGroup.id} has no entry`
    }
  }

  const recordIds = new Set(tabGroupIdsOf(workspace))
  for (const group of workspace.groups) {
    if (group.tabGroup !== null && !recordIds.has(group.tabGroup)) {
      return `entry ${group.id} names tab group ${group.tabGroup}, which has no record`
    }
  }
  return null
}

function focusIsValidAndShowing(workspace: Workspace): string | null {
  const count = workspace.groups.length
  if (count === 0) {
    return null
  }

  const active = workspace.activeGroup
  const validIndex = Number.isInteger(active) && active >= 0 && active < count
  if (!validIndex) {
    return `activeGroup ${active} is not an index into ${count} entries`
  }

  if (!isHidden(workspace, active)) {
    return null
  }
  const indexes = Array.from({ length: count }, (_, index) => index)
  const everyEntryHidden = indexes.every((index) => isHidden(workspace, index))
  if (everyEntryHidden) {
    return null
  }
  return `active entry ${active} is hidden while another entry is showing`
}

function groupHasValidPanes(group: Group): string | null {
  const count = group.panes.length
  if (count < 1 || count > MAX_PANES_PER_GROUP) {
    return `group ${group.id} holds ${count} panes`
  }

  const validActive = Number.isInteger(group.activePane) && group.activePane >= 0
  if (!validActive || group.activePane >= count) {
    return `group ${group.id} has activePane ${group.activePane} among ${count} panes`
  }

  const widths = group.panes.map((pane) => pane.width)
  const sum = widths.reduce((total, width) => total + width, 0)
  if (!(Math.abs(sum - 1) <= WIDTH_TOLERANCE)) {
    return `group ${group.id} widths sum to ${sum}`
  }

  const narrowest = Math.min(...widths)
  if (!(narrowest >= MIN_PANE_WIDTH - WIDTH_TOLERANCE)) {
    return `group ${group.id} has a pane of width ${narrowest}`
  }
  return null
}

function everyGroupHasValidPanes(workspace: Workspace): string | null {
  for (const group of workspace.groups) {
    const broken = groupHasValidPanes(group)
    if (broken !== null) {
      return broken
    }
  }
  return null
}

function idsAreUnique(workspace: Workspace): string | null {
  const duplicatePane = findDuplicate(paneIdsOf(workspace))
  if (duplicatePane !== null) {
    return `pane id ${duplicatePane} appears twice`
  }

  const duplicateGroup = findDuplicate(groupIdsOf(workspace))
  if (duplicateGroup !== null) {
    return `group id ${duplicateGroup} appears twice`
  }
  return null
}

function firstBrokenInvariant(workspace: Workspace): string | null {
  for (const check of CHECKS) {
    const broken = check(workspace)
    if (broken !== null) {
      return `${check.name}: ${broken}`
    }
  }
  return null
}

function runSeed(seed: number): void {
  const random = seededRandom(seed)
  const counters: Counters = { pane: 0, group: 0, tabGroup: 0 }
  const state: FuzzState = { random, counters, workspace: emptyWorkspace() }
  const builders = weightedBuilders()
  const maxEntries = MIN_ENTRY_CAP + (seed % ENTRY_CAP_SPREAD)

  for (let stepNumber = 1; stepNumber <= STEPS_PER_SEED; stepNumber += 1) {
    let builder = pickFrom(random, builders)
    const full = state.workspace.groups.length >= maxEntries
    if (full && GROWING_BUILDERS.includes(builder)) {
      builder = closePaneStep
    }
    const step = builder(state)
    state.workspace = step.apply(state.workspace)

    const broken = firstBrokenInvariant(state.workspace)
    if (broken !== null) {
      throw new Error(`seed ${seed}, step ${stepNumber}, ${step.label}: ${broken}`)
    }
  }
}

describe('workspace model fuzz', () => {
  it.each(SEEDS)('seed %i keeps every invariant', (seed) => {
    expect(() => runSeed(seed)).not.toThrow()
  })
})
