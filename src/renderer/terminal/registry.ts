import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import { Terminal } from '@xterm/xterm'
import type { SessionId } from '@shared/ipc'
import {
  TERMINAL_FONT_FAMILY,
  TERMINAL_FONT_SIZE,
  TERMINAL_SCROLLBACK,
  TERMINAL_THEME
} from './theme'

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

// The shell starts printing the moment main spawns it, which can land before
// createSession has registered a handle to write into. Without somewhere to put
// that output it is simply dropped, and what goes missing is the first prompt.
const pendingOutput = new Map<SessionId, string[]>()

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

export async function createSession(): Promise<SessionId> {
  const id = await window.taqueria.pty.create({ cols: INITIAL_COLS, rows: INITIAL_ROWS })

  const term = new Terminal({
    fontFamily: TERMINAL_FONT_FAMILY,
    fontSize: TERMINAL_FONT_SIZE,
    scrollback: TERMINAL_SCROLLBACK,
    theme: TERMINAL_THEME,
    cursorBlink: true
  })

  const fit = new FitAddon()
  term.loadAddon(fit)

  const element = document.createElement('div')
  element.className = 'terminal-host'

  term.onData((data) => {
    window.taqueria.pty.write(id, data)
  })

  handles.set(id, { id, term, fit, element, opened: false, observer: null })
  drainPendingOutput(id, term)

  return id
}

export function attachSession(id: SessionId, host: HTMLElement): void {
  const handle = handles.get(id)
  if (!handle) {
    return
  }

  // Attaching is also how a terminal moves from one host to another, so undo the
  // previous placement before making the new one.
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
  handle.term.focus()
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

export function sessionTerminal(id: SessionId): Terminal | null {
  const handle = handles.get(id)
  if (!handle) {
    return null
  }
  return handle.term
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
