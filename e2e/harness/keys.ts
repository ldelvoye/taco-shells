import { PageClosedError, type Protocol } from './connection.ts'

// Chromium's modifier bitmask for Input.dispatchKeyEvent.
const MODIFIER_ALT = 1
const MODIFIER_META = 4
const MODIFIER_SHIFT = 8

export interface Key {
  code: string
  text: string
  virtualKeyCode: number
}

export const KEY = {
  b: { code: 'KeyB', text: 'b', virtualKeyCode: 66 },
  t: { code: 'KeyT', text: 't', virtualKeyCode: 84 },
  w: { code: 'KeyW', text: 'w', virtualKeyCode: 87 },
  backslash: { code: 'Backslash', text: '\\', virtualKeyCode: 220 },
  bracketLeft: { code: 'BracketLeft', text: '[', virtualKeyCode: 219 },
  bracketRight: { code: 'BracketRight', text: ']', virtualKeyCode: 221 },
  left: { code: 'ArrowLeft', text: '', virtualKeyCode: 37 },
  right: { code: 'ArrowRight', text: '', virtualKeyCode: 39 },
  comma: { code: 'Comma', text: ',', virtualKeyCode: 188 },
  enter: { code: 'Enter', text: 'Enter', virtualKeyCode: 13 },
  escape: { code: 'Escape', text: '', virtualKeyCode: 27 },
  digit5: { code: 'Digit5', text: '5', virtualKeyCode: 53 }
} satisfies Record<string, Key>

export interface Modifiers {
  cmd?: boolean
  alt?: boolean
  shift?: boolean
}

function modifierMask(modifiers: Modifiers): number {
  let mask = 0
  if (modifiers.cmd) {
    mask += MODIFIER_META
  }
  if (modifiers.alt) {
    mask += MODIFIER_ALT
  }
  if (modifiers.shift) {
    mask += MODIFIER_SHIFT
  }
  return mask
}

// Chromium's own input pipeline rather than a synthetic KeyboardEvent, so a
// key travels the path a real one does, xterm's textarea included.
export async function dispatchKey(
  page: Protocol,
  key: Key,
  modifiers: Modifiers,
  held: boolean
): Promise<void> {
  const stroke = {
    modifiers: modifierMask(modifiers),
    code: key.code,
    key: key.text,
    windowsVirtualKeyCode: key.virtualKeyCode,
    nativeVirtualKeyCode: key.virtualKeyCode,
    autoRepeat: held
  }

  // A key that closes the window takes the page with it, and the rest of the stroke has nowhere to go.
  try {
    await page.send('Input.dispatchKeyEvent', { ...stroke, type: 'rawKeyDown' })
    await page.send('Input.dispatchKeyEvent', { ...stroke, type: 'keyUp' })
  } catch (error) {
    if (error instanceof PageClosedError) {
      return
    }
    throw error
  }
}

export async function press(page: Protocol, key: Key, modifiers: Modifiers = {}): Promise<void> {
  await dispatchKey(page, key, modifiers, false)
}
