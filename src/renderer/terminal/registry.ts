import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import { Terminal, type ITerminalOptions } from '@xterm/xterm'
import type { SessionId } from '@shared/ipc'
import { DEFAULT_SETTINGS, type Settings } from '@shared/settings'
import { DARK_PALETTE, type TerminalColors } from '@shared/theme'

const INITIAL_COLS = 80
const INITIAL_ROWS = 24

interface TerminalHandle {
  id: SessionId
  term: Terminal
  fit: FitAddon
  element: HTMLDivElement
  opened: boolean
  observer: ResizeObserver | null
}

// xterm instances live here rather than in React state. React owns the chrome
// around a terminal; if a re-render ever owned the terminal itself, unmounting
// would take the scrollback and the PTY connection with it.
const handles = new Map<SessionId, TerminalHandle>()

// Sessions created later must match the ones already open, so the current
// options live here rather than being passed in at each call site.
let terminalOptions: ITerminalOptions = optionsFrom(DEFAULT_SETTINGS, DARK_PALETTE.terminal)

// The shell starts printing the moment main spawns it, which can land before
// createSession has registered a handle to write into. Without somewhere to put
// that output it is simply dropped, and what goes missing is the first prompt.
const pendingOutput = new Map<SessionId, string[]>()

type TitleListener = (id: SessionId, title: string) => void
type FocusListener = (id: SessionId) => void

const titleListeners = new Set<TitleListener>()
const focusListeners = new Set<FocusListener>()

window.taqueria.pty.onData((event) => {
  const handle = handles.get(event.id)
  if (handle) {
    handle.term.write(event.data)
    return
  }

  const buffered = pendingOutput.get(event.id)
  if (buffered) {
    buffered.push(event.data)
    return
  }

  pendingOutput.set(event.id, [event.data])
})

export async function createSession(cwdFrom?: SessionId): Promise<SessionId> {
  const size = { cols: INITIAL_COLS, rows: INITIAL_ROWS }
  const id = await window.taqueria.pty.create(size, cwdFrom)

  const term = new Terminal(terminalOptions)

  const fit = new FitAddon()
  term.loadAddon(fit)

  const element = document.createElement('div')
  element.className = 'terminal-host'

  // Clicking into a pane is how focus moves between the panes of a split, so the
  // workspace has to hear about it and not only about the moves it made itself.
  element.addEventListener('focusin', () => {
    emitFocus(id)
  })

  term.onData((data) => {
    window.taqueria.pty.write(id, data)
  })

  // Both title sequences feed the sidebar, and the last one a program sets wins.
  term.onTitleChange((title) => {
    emitTitle(id, title)
  })
  term.parser.registerOscHandler(1, (title) => {
    emitTitle(id, title)
    // False, so xterm's own handler still gets to record the icon name.
    return false
  })

  handles.set(id, { id, term, fit, element, opened: false, observer: null })
  drainPendingOutput(id, term)

  return id
}

export function applyTerminalConfig(settings: Settings, colors: TerminalColors): void {
  terminalOptions = optionsFrom(settings, colors)

  for (const handle of handles.values()) {
    Object.assign(handle.term.options, terminalOptions)
    fitToHost(handle)
  }
}

export function attachSession(id: SessionId, host: HTMLElement): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  // Attaching twice would strand the first ResizeObserver, still connected and
  // still fitting the terminal to a host it has left.
  detachSession(id)
  host.appendChild(handle.element)

  if (!handle.opened) {
    handle.term.open(handle.element)
    loadWebglRenderer(handle.term)
    handle.opened = true
  }

  const observer = new ResizeObserver(() => {
    fitToHost(handle)
  })
  observer.observe(host)
  handle.observer = observer

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

  handle.element.remove()
}

export function disposeSession(id: SessionId): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  detachSession(id)
  handle.term.dispose()
  handles.delete(id)
  pendingOutput.delete(id)
  window.taqueria.pty.kill(id)
}

export function focusSession(id: SessionId): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  handle.term.focus()
}

export function sessionTerminal(id: SessionId): Terminal | null {
  const handle = handles.get(id)
  if (!handle) {
    return null
  }
  return handle.term
}

export function onSessionTitle(listener: TitleListener): () => void {
  titleListeners.add(listener)
  return () => {
    titleListeners.delete(listener)
  }
}

export function onSessionFocus(listener: FocusListener): () => void {
  focusListeners.add(listener)
  return () => {
    focusListeners.delete(listener)
  }
}

function emitTitle(id: SessionId, title: string): void {
  for (const listener of titleListeners) {
    listener(id, title)
  }
}

function emitFocus(id: SessionId): void {
  for (const listener of focusListeners) {
    listener(id)
  }
}

function drainPendingOutput(id: SessionId, term: Terminal): void {
  const buffered = pendingOutput.get(id)
  if (!buffered) {
    return
  }

  pendingOutput.delete(id)
  for (const chunk of buffered) {
    term.write(chunk)
  }
}

function optionsFrom(settings: Settings, colors: TerminalColors): ITerminalOptions {
  return {
    fontFamily: settings.fontFamily,
    fontSize: settings.fontSize,
    scrollback: settings.scrollback,
    scrollSensitivity: settings.scrollSensitivity,
    cursorBlink: settings.cursorBlink,
    cursorStyle: settings.cursorStyle,
    theme: colors
  }
}

function fitToHost(handle: TerminalHandle): void {
  const proposed = handle.fit.proposeDimensions()
  if (!proposed) {
    return
  }
  if (!Number.isFinite(proposed.cols) || !Number.isFinite(proposed.rows)) {
    return
  }
  if (proposed.cols === handle.term.cols && proposed.rows === handle.term.rows) {
    return
  }

  handle.fit.fit()
  window.taqueria.pty.resize(handle.id, { cols: handle.term.cols, rows: handle.term.rows })
}

function loadWebglRenderer(term: Terminal): void {
  try {
    const addon = new WebglAddon()
    addon.onContextLoss(() => {
      addon.dispose()
    })
    term.loadAddon(addon)
  } catch {
    // No WebGL context available; xterm keeps its DOM renderer.
  }
}
