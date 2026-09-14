import { describe, expect, it } from 'vitest'

import type { Keymap } from './keys'
import { codeToKey, formatChord, parseChord, resolveKeymap } from './keys'

describe('formatChord', () => {
  it('canonicalises differently-spelled chords to the same string', () => {
    const variants = ['Cmd+Shift+A', 'shift+cmd+a', 'meta+shift+A']
    for (const variant of variants) {
      const chord = parseChord(variant)
      if (chord === null) {
        throw new Error(`expected "${variant}" to parse`)
      }
      expect(formatChord(chord)).toBe('cmd+shift+a')
    }
  })
})

describe('parseChord', () => {
  it('rejects malformed chord strings', () => {
    const malformed = ['cmd', 'cmd+', 'foo+a', 'cmd+cmd+a', 'cmd+abc']
    for (const input of malformed) {
      expect(parseChord(input)).toBeNull()
    }
  })
})

describe('codeToKey', () => {
  it('returns null for modifier codes', () => {
    const modifierCodes = ['MetaLeft', 'ShiftRight', 'AltLeft', 'ControlLeft']
    for (const code of modifierCodes) {
      expect(codeToKey(code)).toBeNull()
    }
  })
})

describe('resolveKeymap', () => {
  it('lets a user binding override a default spelled differently', () => {
    const defaults: Keymap = { 'cmd+k': 'terminal.clear' }
    const user: Keymap = { 'Cmd+K': 'terminal.copy' }

    const resolution = resolveKeymap(defaults, user)

    expect(resolution.bindings.get('cmd+k')).toBe('terminal.copy')
  })

  it('lets a user null unbind a default', () => {
    const defaults: Keymap = { 'cmd+k': 'terminal.clear' }
    const user: Keymap = { 'cmd+k': null }

    const resolution = resolveKeymap(defaults, user)

    expect(resolution.bindings.has('cmd+k')).toBe(false)
  })

  it('skips a malformed chord and an unknown command id, reporting both and keeping other bindings', () => {
    const defaults = {
      'cmd+c': 'terminal.copy',
      'cmd+': 'terminal.paste',
      'cmd+k': 'not.a.command'
    } as unknown as Keymap

    const resolution = resolveKeymap(defaults)

    expect(resolution.bindings.get('cmd+c')).toBe('terminal.copy')
    expect(resolution.bindings.size).toBe(1)
    expect(resolution.problems).toHaveLength(2)
  })
})
