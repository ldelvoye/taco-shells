import type { PtySize, SessionId } from '@protocol/ipc'
import type { TerminalEvent } from '@protocol/runtimeMessages'
import type { Settings } from '@protocol/settings'
import type { TerminalColors } from '@protocol/theme'
import { DEFAULT_SETTINGS } from '@settings/settings'
import { DARK_PALETTE } from '@settings/theme'
import { makeXtermView } from './engine/xtermView'
import { afterShowing, viewed, type Recency } from './shown'
import type { TerminalView } from './view'

interface TerminalEntry {
  id: SessionId
  host: HTMLElement | null
  view: TerminalView | null
  /** The number of this view's attach. Until `attached` comes back with it, what arrives was meant for a view that is gone. */
  awaiting: number | null
  observer: ResizeObserver | null
  /** The size last asked of the runtime, so the same one is not asked for twice. Null until one is known. */
  asked: PtySize | null
  ended: boolean
}

// Views live here rather than in React state. React owns the chrome around a
// terminal; if a re-render ever owned the view itself, unmounting would throw it
// away and the next mount would load the whole terminal again.
const terminals = new Map<SessionId, TerminalEntry>()

let recency: Recency = []

let attaches = 0

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
  const entry = terminals.get(event.id)
  if (!entry) {
    return
  }

  if (event.type === 'end') {
    entry.ended = true
    disposeView(entry)
    return
  }

  const view = entry.view
  if (view === null) {
    return
  }

  if (event.type === 'attached') {
    if (event.view !== entry.awaiting) {
      return
    }
    entry.awaiting = null
    view.load(event.snapshot, event.size)
    if (entry.asked === null) {
      entry.asked = event.size
    }
    fitHosts()
    return
  }
  if (entry.awaiting !== null) {
    return
  }
  if (event.type === 'output') {
    view.write(event.data)
    return
  }
  if (event.type === 'resized') {
    view.resize(event.size)
    return
  }
  if (event.type === 'cleared') {
    view.clear()
  }
}

function entryFor(id: SessionId): TerminalEntry {
  const existing = terminals.get(id)
  if (existing) {
    return existing
  }

  const entry: TerminalEntry = {
    id,
    host: null,
    view: null,
    awaiting: null,
    observer: null,
    asked: null,
    ended: false
  }
  terminals.set(id, entry)
  return entry
}

function createView(id: SessionId): TerminalView {
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

  return view
}

function giveView(entry: TerminalEntry): void {
  const view = createView(entry.id)
  entry.view = view

  const host = entry.host
  if (host !== null) {
    host.appendChild(view.element)
    view.mount()
  }

  // Null when the host has no layout yet: the runtime then keeps the size the terminal has.
  const size = measure(entry)
  if (size !== null) {
    entry.asked = size
  }
  attaches += 1
  entry.awaiting = attaches
  window.tacoShells.pty.attach(entry.id, size, attaches)
}

function disposeView(entry: TerminalEntry): void {
  const view = entry.view
  if (view === null) {
    return
  }

  view.element.remove()
  view.dispose()
  entry.view = null
  entry.awaiting = null
}

function stopObserving(entry: TerminalEntry): void {
  if (entry.observer) {
    entry.observer.disconnect()
    entry.observer = null
  }
}

export function applyTerminalConfig(settings: Settings, colors: TerminalColors): void {
  currentSettings = settings
  currentColors = colors

  for (const entry of terminals.values()) {
    if (entry.view !== null) {
      entry.view.apply(settings, colors)
    }
  }
  fitHosts()
}

export function attachSession(id: SessionId, host: HTMLElement): void {
  const entry = entryFor(id)

  // Attaching twice would strand the first ResizeObserver, still connected and
  // still asking for the size of a host the view has left.
  detachSession(id)
  entry.host = host
  if (entry.view !== null) {
    host.appendChild(entry.view.element)
    entry.view.mount()
  }

  const observer = new ResizeObserver(() => {
    fitHost(entry)
  })
  observer.observe(host)
  entry.observer = observer

  fitHost(entry)
  // No focus here: every session attaches on mount, active or not, so focusing
  // on attach would let a background terminal steal the keyboard.
}

export function detachSession(id: SessionId): void {
  const entry = terminals.get(id)
  if (!entry) {
    return
  }

  stopObserving(entry)
  entry.host = null
  if (entry.view !== null) {
    entry.view.element.remove()
  }
}

// A terminal's end mark and its pane leaving the workspace arrive in either order,
// and a terminal that ended before its view attached sends no end mark at all.
export function showSessions(onScreen: SessionId[], open: ReadonlySet<SessionId>): void {
  const shown = afterShowing(recency, onScreen)
  recency = shown.filter((id) => open.has(id))
  const wanted = viewed(recency, onScreen, open)

  const held = [...terminals.values()]
  for (const entry of held) {
    if (!open.has(entry.id)) {
      stopObserving(entry)
      disposeView(entry)
      terminals.delete(entry.id)
      continue
    }
    if (entry.view !== null && !wanted.has(entry.id)) {
      window.tacoShells.pty.detach(entry.id)
      disposeView(entry)
    }
  }

  for (const id of wanted) {
    const entry = entryFor(id)
    if (entry.view === null && !entry.ended) {
      giveView(entry)
    }
  }

  fitHosts()
}

export function focusSession(id: SessionId): void {
  const entry = terminals.get(id)
  if (!entry || entry.view === null) {
    return
  }

  const alreadyFocused = entry.view.element.contains(document.activeElement)
  if (!alreadyFocused) {
    expectedFocus = id
  }
  entry.view.focus()
}

export function sessionView(id: SessionId): TerminalView | null {
  const entry = terminals.get(id)
  if (!entry) {
    return null
  }
  return entry.view
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

function measure(entry: TerminalEntry): PtySize | null {
  const host = entry.host
  if (host === null) {
    return null
  }

  if (entry.view !== null) {
    const own = entry.view.sizeThatFits(host)
    if (own !== null) {
      return own
    }
  }

  for (const other of terminals.values()) {
    if (other === entry || other.view === null) {
      continue
    }
    const size = other.view.sizeThatFits(host)
    if (size !== null) {
      return size
    }
  }
  return null
}

function fitHost(entry: TerminalEntry): void {
  const proposed = measure(entry)
  if (!proposed) {
    return
  }
  if (sameSize(entry.asked, proposed)) {
    return
  }

  entry.asked = proposed
  window.tacoShells.pty.resize(entry.id, proposed)
}

function fitHosts(): void {
  for (const entry of terminals.values()) {
    fitHost(entry)
  }
}
