import type { CommandId } from './commands'
import { isCommandId } from './commands'

export interface Chord {
  cmd: boolean
  ctrl: boolean
  alt: boolean
  shift: boolean
  key: string
}

/** A keydown reduced to the fields chord resolution needs (a DOM KeyboardEvent satisfies it structurally). */
export interface KeyStroke {
  code: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
}

export type Keymap = Record<string, CommandId | null>

export interface KeymapResolution {
  bindings: ReadonlyMap<string, CommandId>
  problems: string[]
}

type ModifierKey = 'cmd' | 'ctrl' | 'alt' | 'shift'

const MODIFIER_ALIASES: Map<string, ModifierKey> = new Map([
  ['cmd', 'cmd'],
  ['command', 'cmd'],
  ['meta', 'cmd'],
  ['ctrl', 'ctrl'],
  ['control', 'ctrl'],
  ['alt', 'alt'],
  ['option', 'alt'],
  ['opt', 'alt'],
  ['shift', 'shift']
])

const NAMED_KEYS: ReadonlySet<string> = new Set([
  'space',
  'enter',
  'tab',
  'escape',
  'backspace',
  'delete',
  'up',
  'down',
  'left',
  'right',
  'home',
  'end',
  'pageup',
  'pagedown',
  'f1',
  'f2',
  'f3',
  'f4',
  'f5',
  'f6',
  'f7',
  'f8',
  'f9',
  'f10',
  'f11',
  'f12'
])

const LETTER_CODE_PATTERN = /^Key([A-Z])$/
const DIGIT_CODE_PATTERN = /^Digit([0-9])$/
const FUNCTION_CODE_PATTERN = /^F([1-9]|1[0-2])$/

const CODE_TO_KEY: Map<string, string> = new Map([
  ['Space', 'space'],
  ['Enter', 'enter'],
  ['NumpadEnter', 'enter'],
  ['Tab', 'tab'],
  ['Escape', 'escape'],
  ['Backspace', 'backspace'],
  ['Delete', 'delete'],
  ['ArrowUp', 'up'],
  ['ArrowDown', 'down'],
  ['ArrowLeft', 'left'],
  ['ArrowRight', 'right'],
  ['Home', 'home'],
  ['End', 'end'],
  ['PageUp', 'pageup'],
  ['PageDown', 'pagedown'],
  ['Minus', '-'],
  ['Equal', '='],
  ['BracketLeft', '['],
  ['BracketRight', ']'],
  ['Backslash', '\\'],
  ['Semicolon', ';'],
  ['Quote', "'"],
  ['Backquote', '`'],
  ['Comma', ','],
  ['Period', '.'],
  ['Slash', '/']
])

// A literal "+" can't be a base key: splitting on "+" turns it into an empty segment. Write it as shift+=.
export function parseChord(input: string): Chord | null {
  const segments = input.split('+')
  const hasEmptySegment = segments.some((segment) => segment === '')
  if (hasEmptySegment) {
    return null
  }

  const lastIndex = segments.length - 1
  const modifierSegments = segments.slice(0, lastIndex)
  const baseSegment = segments[lastIndex]

  const chord: Chord = { cmd: false, ctrl: false, alt: false, shift: false, key: '' }

  for (const segment of modifierSegments) {
    const alias = segment.toLowerCase()
    const modifierField = MODIFIER_ALIASES.get(alias)
    if (modifierField === undefined) {
      return null
    }
    if (chord[modifierField]) {
      return null
    }
    chord[modifierField] = true
  }

  const baseKey = baseSegment.toLowerCase()
  const baseKeyIsModifierAlias = MODIFIER_ALIASES.has(baseKey)
  if (baseKeyIsModifierAlias) {
    return null
  }

  const baseKeyIsUnknownMultiChar = baseKey.length > 1 && !NAMED_KEYS.has(baseKey)
  if (baseKeyIsUnknownMultiChar) {
    return null
  }

  chord.key = baseKey
  return chord
}

export function formatChord(chord: Chord): string {
  const modifiers: string[] = []
  if (chord.cmd) {
    modifiers.push('cmd')
  }
  if (chord.ctrl) {
    modifiers.push('ctrl')
  }
  if (chord.alt) {
    modifiers.push('alt')
  }
  if (chord.shift) {
    modifiers.push('shift')
  }

  const parts = [...modifiers, chord.key]
  return parts.join('+')
}

// We read code, not key, because macOS rewrites KeyboardEvent.key when Option is held (alt+b arrives as a different glyph).
export function codeToKey(code: string): string | null {
  const letterMatch = LETTER_CODE_PATTERN.exec(code)
  if (letterMatch !== null) {
    const letter = letterMatch[1]
    return letter.toLowerCase()
  }

  const digitMatch = DIGIT_CODE_PATTERN.exec(code)
  if (digitMatch !== null) {
    const digit = digitMatch[1]
    return digit
  }

  const isFunctionKey = FUNCTION_CODE_PATTERN.test(code)
  if (isFunctionKey) {
    return code.toLowerCase()
  }

  const mapped = CODE_TO_KEY.get(code)
  if (mapped !== undefined) {
    return mapped
  }

  return null
}

export function chordFromKeyStroke(stroke: KeyStroke): Chord | null {
  const key = codeToKey(stroke.code)
  if (key === null) {
    return null
  }

  return {
    cmd: stroke.metaKey,
    ctrl: stroke.ctrlKey,
    alt: stroke.altKey,
    shift: stroke.shiftKey,
    key
  }
}

function applyKeymapLayer(
  layer: Keymap,
  bindings: Map<string, CommandId>,
  problems: string[]
): void {
  const entries = Object.entries(layer)
  for (const [chordText, commandId] of entries) {
    const chord = parseChord(chordText)
    if (chord === null) {
      problems.push(`"${chordText}" is not a valid chord`)
      continue
    }
    const canonical = formatChord(chord)

    if (commandId === null) {
      bindings.delete(canonical)
      continue
    }

    if (!isCommandId(commandId)) {
      problems.push(`"${chordText}": "${commandId}" is not a known command id`)
      continue
    }

    bindings.set(canonical, commandId)
  }
}

export function resolveKeymap(defaults: Keymap, user?: Keymap): KeymapResolution {
  const bindings = new Map<string, CommandId>()
  const problems: string[] = []

  applyKeymapLayer(defaults, bindings, problems)
  if (user !== undefined) {
    applyKeymapLayer(user, bindings, problems)
  }

  return { bindings, problems }
}

export function commandForKeyStroke(
  bindings: ReadonlyMap<string, CommandId>,
  stroke: KeyStroke
): CommandId | null {
  const chord = chordFromKeyStroke(stroke)
  if (chord === null) {
    return null
  }

  const canonical = formatChord(chord)
  const commandId = bindings.get(canonical)
  if (commandId === undefined) {
    return null
  }

  return commandId
}
