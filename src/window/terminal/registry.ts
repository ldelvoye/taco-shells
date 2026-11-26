import type { PtySize, SessionId } from '@protocol/ipc'
import type { TerminalEvent } from '@protocol/runtimeMessages'
import type { Settings } from '@protocol/settings'
import type { TerminalColors } from '@protocol/theme'
import { DEFAULT_SETTINGS } from '@settings/settings'
import { DARK_PALETTE } from '@settings/theme'
import { makeXtermView } from './engine/xtermView'
import type { TerminalView } from './view'

interface TerminalHandle {
  id: SessionId
  view: TerminalView
  observer: ResizeObserver | null
  /** The size last asked of the runtime, so the same one is not asked for twice. Null until one is known. */
  asked: PtySize | null
  attachSent: boolean
}

// Views live here rather than in React state. React owns the chrome around a
// terminal; if a re-render ever owned the view itself, unmounting would throw it
// away and the next mount would load the whole terminal again.
const handles = new Map<SessionId, TerminalHandle>()

// The session focusSession has just moved focus to. It is set there and consumed
// by the focusin listener below, which is the only pair that reads it.
let expectedFocus: SessionId | null = null

// Sessions created later must match the ones already open, so the current
// options live here rather than being passed in at each call site.
let currentSettings: Settings = DEFAULT_SETTINGS
let currentColors: TerminalColors = DARK_PALETTE.terminal

type FocusListener = (id: SessionId) => void

const focusListeners = new Set<FocusListener>()

window.tacoShells.pty.onEvent(handleEvent)

function handleEvent(event: TerminalEvent): void {
  const handle = handles.get(event.id)
  if (!handle) {
    return
  }

  if (event.type === 'attached') {
    handle.view.load(event.snapshot, event.size)
    if (handle.asked === null) {
      handle.asked = event.size
    }
    return
  }
  if (event.type === 'output') {
    handle.view.write(event.data)
    return
  }
  if (event.type === 'resized') {
    handle.view.resize(event.size)
    return
  }
  if (event.type === 'cleared') {
    handle.view.clear()
    return
  }
  if (event.type === 'end') {
    disposeSession(event.id)
  }
}

function handleFor(id: SessionId): TerminalHandle {
  const existing = handles.get(id)
  if (existing) {
    return existing
  }
  return createHandle(id)
}

function createHandle(id: SessionId): TerminalHandle {
  const view = makeXtermView({
    settings: currentSettings,
    colors: currentColors,
    onInput: (data) => {
      window.tacoShells.pty.write(id, data)
    },
    onLink: openLinkOnCmdClick
  })

  // Clicking into a pane is how focus moves between the panes of a split, so the
  // workspace has to hear about it. What arrives here is either that click or the
  // echo of a focus the app itself asked for, and only the click is news: by the
  // time an echo lands the workspace may have moved on, and reporting it would
  // drag the active pane back.
  view.element.addEventListener('focusin', () => {
    if (expectedFocus === id) {
      expectedFocus = null
      return
    }
    emitFocus(id)
  })

  const handle: TerminalHandle = { id, view, observer: null, asked: null, attachSent: false }
  handles.set(id, handle)

  return handle
}

export function applyTerminalConfig(settings: Settings, colors: TerminalColors): void {
  currentSettings = settings
  currentColors = colors

  for (const handle of handles.values()) {
    handle.view.apply(settings, colors)
    fitToHost(handle)
  }
}

export function attachSession(id: SessionId, host: HTMLElement): void {
  const handle = handleFor(id)

  // Attaching twice would strand the first ResizeObserver, still connected and
  // still asking for the size of a host the view has left.
  detachSession(id)
  host.appendChild(handle.view.element)
  handle.view.mount()

  const observer = new ResizeObserver(() => {
    fitToHost(handle)
  })
  observer.observe(host)
  handle.observer = observer

  if (!handle.attachSent) {
    // Null when the host has no layout yet: the runtime then keeps the size the terminal has.
    const size = handle.view.fittingSize()
    handle.attachSent = true
    handle.asked = size
    window.tacoShells.pty.attach(id, size)
  }

  fitToHost(handle)
  // No focus here: every session attaches on mount, active or not, so focusing
  // on attach would let a background terminal steal the keyboard.
}

export function detachSession(id: SessionId): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  if (handle.observer) {
    handle.observer.disconnect()
    handle.observer = null
  }

  handle.view.element.remove()
}

export function disposeSession(id: SessionId): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  detachSession(id)
  handle.view.dispose()
  handles.delete(id)
}

// A terminal's end mark and its pane leaving the workspace arrive in either order,
// and a terminal that ended before its view attached sends no end mark at all.
export function disposeSessionsNotIn(open: ReadonlySet<SessionId>): void {
  const held = [...handles.keys()]
  for (const id of held) {
    if (!open.has(id)) {
      disposeSession(id)
    }
  }
}

export function focusSession(id: SessionId): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  const alreadyFocused = handle.view.element.contains(document.activeElement)
  if (!alreadyFocused) {
    expectedFocus = id
  }
  handle.view.focus()
}

export function sessionView(id: SessionId): TerminalView | null {
  const handle = handles.get(id)
  if (!handle) {
    return null
  }
  return handle.view
}

export function onSessionFocus(listener: FocusListener): () => void {
  focusListeners.add(listener)
  return () => {
    focusListeners.delete(listener)
  }
}

function emitFocus(id: SessionId): void {
  for (const listener of focusListeners) {
    listener(id)
  }
}

// A plain click has to stay free for selecting text, so a link only opens with
// Cmd held.
function openLinkOnCmdClick(event: MouseEvent, url: string): void {
  if (!event.metaKey) {
    return
  }
  window.tacoShells.link.open(url)
}

function sameSize(first: PtySize | null, second: PtySize): boolean {
  if (first === null) {
    return false
  }
  return first.cols === second.cols && first.rows === second.rows
}

function fitToHost(handle: TerminalHandle): void {
  const proposed = handle.view.fittingSize()
  if (!proposed) {
    return
  }
  if (sameSize(handle.asked, proposed)) {
    return
  }

  handle.asked = proposed
  window.tacoShells.pty.resize(handle.id, proposed)
}
