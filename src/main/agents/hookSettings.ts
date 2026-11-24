import { join } from 'node:path'
import { applyEdits, modify, type FormattingOptions, type JSONPath } from 'jsonc-parser'
import type { HookWiring } from '@shared/agentSetup'
import type { HarnessSetup, HookEntry } from '@shared/harness/types'
import { parseJsonc } from '../config/jsonc'

const EMPTY_FILE = '{}'
const DEFAULT_INDENT = '  '
const FIRST_INDENTED_LINE = /^([ \t]+)\S/m
const SHELL_WORD_BREAK = /["'\s]+/

type JsonRecord = Record<string, unknown>

export interface HookSettingsResult {
  text: string | null
  problem: string | null
}

interface HookRef {
  event: string
  groupIndex: number
  hookIndex: number
  command: string
  hooksInGroup: number
  groupsInEvent: number
}

interface HooksRead {
  hooks: JsonRecord | null
  problem: string | null
}

type CommandTest = (command: string) => boolean

function refusal(problem: string): HookSettingsResult {
  return { text: null, problem }
}

function unchanged(): HookSettingsResult {
  return { text: null, problem: null }
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function readHooks(text: string, label: string): HooksRead {
  const parsed = parseJsonc(text, label)
  const root = parsed.value
  if (parsed.problems.length > 0 || root === null) {
    return { hooks: null, problem: `${label} could not be read as JSON, so nothing was changed.` }
  }
  if (!('hooks' in root)) {
    return { hooks: null, problem: null }
  }
  const hooks = root.hooks
  if (!isRecord(hooks)) {
    return { hooks: null, problem: `${label}: "hooks" must be an object, so nothing was changed.` }
  }
  return { hooks, problem: null }
}

function hookRefsOf(hooks: JsonRecord | null): HookRef[] {
  const refs: HookRef[] = []
  if (hooks === null) {
    return refs
  }

  const events = Object.entries(hooks)
  for (const [event, groups] of events) {
    if (!Array.isArray(groups)) {
      continue
    }
    const groupsInEvent = groups.length
    groups.forEach((group: unknown, groupIndex) => {
      if (!isRecord(group) || !Array.isArray(group.hooks)) {
        return
      }
      const groupHooks: unknown[] = group.hooks
      const hooksInGroup = groupHooks.length
      groupHooks.forEach((hook, hookIndex) => {
        if (!isRecord(hook) || typeof hook.command !== 'string') {
          return
        }
        refs.push({ event, groupIndex, hookIndex, command: hook.command, hooksInGroup, groupsInEvent })
      })
    })
  }
  return refs
}

function commandFor(entry: HookEntry, hooksDirectory: string): string {
  const path = join(hooksDirectory, entry.script)
  if (path.includes(' ')) {
    return `"${path}"`
  }
  return path
}

// Managed: the command runs a file in the app's hooks directory. By hand: it
// runs one of the setup's scripts from anywhere else. These decide what removal deletes.
function managedTest(hooksDirectory: string): CommandTest {
  const prefix = `${hooksDirectory}/`
  const quotedPrefix = `"${prefix}`
  return (command) => command.startsWith(prefix) || command.startsWith(quotedPrefix)
}

function isScriptPath(word: string, scripts: readonly string[]): boolean {
  return scripts.some((script) => word === script || word.endsWith(`/${script}`))
}

function handTest(setup: HarnessSetup, hooksDirectory: string): CommandTest {
  const isManaged = managedTest(hooksDirectory)
  const scripts = setup.entries.map((entry) => entry.script)
  return (command) => {
    if (isManaged(command)) {
      return false
    }
    const words = command.split(SHELL_WORD_BREAK)
    return words.some((word) => isScriptPath(word, scripts))
  }
}

// Insertions are indented the way the file already is, so they do not stand out in it.
function formattingFor(text: string): FormattingOptions {
  const indented = FIRST_INDENTED_LINE.exec(text)
  let indent = DEFAULT_INDENT
  if (indented !== null) {
    indent = indented[1]
  }

  if (indent.startsWith('\t')) {
    return { insertSpaces: false, tabSize: 1 }
  }
  return { insertSpaces: true, tabSize: indent.length }
}

function edit(text: string, path: JSONPath, value: unknown, formattingOptions: FormattingOptions): string {
  // Without the flag, a value at an array index replaces the element there instead of going in before it.
  const isArrayInsertion = typeof path[path.length - 1] === 'number' && value !== undefined
  const edits = modify(text, path, value, { formattingOptions, isArrayInsertion })
  return applyEdits(text, edits)
}

function groupFor(entry: HookEntry, hooksDirectory: string): JsonRecord {
  const command = commandFor(entry, hooksDirectory)
  const hook: JsonRecord = { type: 'command', command }
  if (entry.async) {
    hook.async = true
  }
  return { hooks: [hook] }
}

function unlistedEventProblem(
  hooks: JsonRecord | null,
  entries: readonly HookEntry[],
  label: string
): string | null {
  if (hooks === null) {
    return null
  }
  for (const entry of entries) {
    const groups = hooks[entry.event]
    if (groups !== undefined && !Array.isArray(groups)) {
      return `${label}: "hooks.${entry.event}" must be a list, so nothing was changed.`
    }
  }
  return null
}

// The narrowest path that removes the hook without leaving an empty group, event or "hooks".
function removalPathOf(ref: HookRef, eventCount: number): JSONPath {
  if (ref.hooksInGroup > 1) {
    return ['hooks', ref.event, ref.groupIndex, 'hooks', ref.hookIndex]
  }
  if (ref.groupsInEvent > 1) {
    return ['hooks', ref.event, ref.groupIndex]
  }
  if (eventCount > 1) {
    return ['hooks', ref.event]
  }
  return ['hooks']
}

function removeMatching(text: string, label: string, test: CommandTest): HookSettingsResult {
  const formattingOptions = formattingFor(text)
  let edited = text
  let remaining = Infinity

  // One hook per pass, read again each time: every removal shifts the indexes after it.
  while (true) {
    const read = readHooks(edited, label)
    if (read.problem !== null) {
      return refusal(read.problem)
    }
    const hooks = read.hooks
    if (hooks === null) {
      break
    }
    const refs = hookRefsOf(hooks)
    const matches = refs.filter((ref) => test(ref.command))
    if (matches.length === 0) {
      break
    }
    if (matches.length >= remaining) {
      return refusal(`${label} could not be edited, so nothing was changed.`)
    }
    remaining = matches.length

    const events = Object.keys(hooks)
    const path = removalPathOf(matches[0], events.length)
    edited = edit(edited, path, undefined, formattingOptions)
  }

  if (edited === text) {
    return unchanged()
  }
  return { text: edited, problem: null }
}

export function entryKey(entry: HookEntry): string {
  return `${entry.event}:${entry.script}`
}

export function hookWiringOf(text: string | null, setup: HarnessSetup, hooksDirectory: string): HookWiring {
  if (text === null) {
    return 'none'
  }

  const read = readHooks(text, setup.settingsFile)
  const refs = hookRefsOf(read.hooks)
  const isHand = handTest(setup, hooksDirectory)
  const isManaged = managedTest(hooksDirectory)
  if (refs.some((ref) => isHand(ref.command))) {
    return 'byHand'
  }
  if (refs.some((ref) => isManaged(ref.command))) {
    return 'managed'
  }
  return 'none'
}

export function addEntries(
  text: string | null,
  entries: readonly HookEntry[],
  hooksDirectory: string,
  label: string
): HookSettingsResult {
  let source = EMPTY_FILE
  let isNewFile = true
  if (text !== null && text.trim() !== '') {
    source = text
    isNewFile = false
  }

  const read = readHooks(source, label)
  if (read.problem !== null) {
    return refusal(read.problem)
  }
  const eventProblem = unlistedEventProblem(read.hooks, entries, label)
  if (eventProblem !== null) {
    return refusal(eventProblem)
  }

  const refs = hookRefsOf(read.hooks)
  const missing = entries.filter((entry) => {
    const command = commandFor(entry, hooksDirectory)
    return !refs.some((ref) => ref.event === entry.event && ref.command === command)
  })
  if (missing.length === 0) {
    return unchanged()
  }

  const formattingOptions = formattingFor(source)
  const groupCounts = new Map<string, number>()
  if (read.hooks !== null) {
    const events = Object.entries(read.hooks)
    for (const [event, groups] of events) {
      if (Array.isArray(groups)) {
        groupCounts.set(event, groups.length)
      }
    }
  }

  let edited = source
  for (const entry of missing) {
    const group = groupFor(entry, hooksDirectory)
    const groupCount = groupCounts.get(entry.event)
    if (groupCount === undefined) {
      edited = edit(edited, ['hooks', entry.event], [group], formattingOptions)
      groupCounts.set(entry.event, 1)
    } else {
      edited = edit(edited, ['hooks', entry.event, groupCount], group, formattingOptions)
      groupCounts.set(entry.event, groupCount + 1)
    }
  }

  if (isNewFile) {
    edited = `${edited}\n`
  }
  return { text: edited, problem: null }
}

export function removeManagedEntries(
  text: string,
  hooksDirectory: string,
  label: string
): HookSettingsResult {
  const test = managedTest(hooksDirectory)
  return removeMatching(text, label, test)
}

export function removeHandEntries(
  text: string,
  setup: HarnessSetup,
  hooksDirectory: string,
  label: string
): HookSettingsResult {
  const test = handTest(setup, hooksDirectory)
  return removeMatching(text, label, test)
}
