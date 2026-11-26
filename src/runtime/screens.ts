import type { PtySize, SessionId } from '@protocol/ipc'
import type { TerminalEvent, TerminalRequest } from '@protocol/runtimeMessages'
import type { MakeScreen, Screen } from './screen'
import type { PtySessions } from './sessions'

export const FLOOD_LIMIT = 1_000_000
const DEFAULT_SCROLLBACK = 1000

export interface ScreensOptions {
  makeScreen: MakeScreen
  /** What the screens drive in the shells. */
  shells: Pick<PtySessions, 'write' | 'resize' | 'pause' | 'resume'>
}

type TitleListener = (id: SessionId, title: string) => void

interface Connection {
  post: (event: TerminalEvent) => void
}

interface View {
  connection: Connection
  attached: boolean
  kept: TerminalEvent[]
}

interface TerminalState {
  screen: Screen
  size: PtySize
  shownSize: PtySize
  unparsed: number
  paused: boolean
  ending: boolean
  view: View | null
}

function sameSize(first: PtySize, second: PtySize): boolean {
  return first.cols === second.cols && first.rows === second.rows
}

export class Screens {
  private readonly terminals = new Map<SessionId, TerminalState>()
  private readonly titleListeners = new Set<TitleListener>()
  private readonly makeScreen: MakeScreen
  private readonly shells: ScreensOptions['shells']
  private scrollback = DEFAULT_SCROLLBACK
  private current: Connection | null = null

  constructor(options: ScreensOptions) {
    this.makeScreen = options.makeScreen
    this.shells = options.shells
  }

  open(id: SessionId, size: PtySize): void {
    const screen = this.makeScreen({ size, scrollback: this.scrollback })
    screen.onTitle((title) => {
      for (const listener of this.titleListeners) {
        listener(id, title)
      }
    })
    // The screen answers for the terminal, since a view is not always there to.
    screen.onReply((data) => {
      this.shells.write(id, data)
    })
    this.terminals.set(id, {
      screen,
      size,
      shownSize: size,
      unparsed: 0,
      paused: false,
      ending: false,
      view: null
    })
  }

  output(id: SessionId, data: string): void {
    const terminal = this.terminals.get(id)
    if (terminal === undefined) {
      return
    }

    terminal.unparsed += data.length
    if (terminal.unparsed > FLOOD_LIMIT && !terminal.paused) {
      terminal.paused = true
      this.shells.pause(id)
    }
    terminal.screen.write(data, () => {
      terminal.unparsed -= data.length
      this.resumeWhenDrained(id, terminal)
    })
    this.send(terminal, { type: 'output', id, data })
  }

  ended(id: SessionId, exitCode: number): void {
    const terminal = this.terminals.get(id)
    if (terminal === undefined || terminal.ending) {
      return
    }

    terminal.ending = true
    terminal.screen.afterParsed(() => {
      const view = terminal.view
      if (view !== null && view.attached) {
        view.connection.post({ type: 'end', id, exitCode })
      }
      terminal.screen.dispose()
      this.terminals.delete(id)
    })
  }

  resize(id: SessionId, size: PtySize): void {
    const terminal = this.terminals.get(id)
    if (terminal === undefined) {
      return
    }
    this.resizeTerminal(id, terminal, size)
  }

  setScrollback(lines: number): void {
    this.scrollback = lines
    for (const terminal of this.terminals.values()) {
      terminal.screen.setScrollback(lines)
    }
  }

  onTitle(listener: TitleListener): void {
    this.titleListeners.add(listener)
  }

  /** A window's connection. A second call replaces the first: every view of the first is forgotten. */
  connect(post: (event: TerminalEvent) => void): (request: TerminalRequest) => void {
    const connection: Connection = { post }
    this.current = connection
    for (const terminal of this.terminals.values()) {
      terminal.view = null
    }

    return (request) => {
      if (this.current !== connection) {
        return
      }
      this.handle(connection, request)
    }
  }

  private handle(connection: Connection, request: TerminalRequest): void {
    if (request.type === 'write') {
      this.shells.write(request.id, request.data)
      return
    }
    if (request.type === 'resize') {
      this.resize(request.id, request.size)
      return
    }
    if (request.type === 'clear') {
      this.clear(request.id)
      return
    }
    if (request.type === 'detach') {
      this.detach(request.id)
      return
    }
    this.attach(connection, request.id, request.size)
  }

  private attach(connection: Connection, id: SessionId, size: PtySize | null): void {
    const terminal = this.terminals.get(id)
    if (terminal === undefined || terminal.ending) {
      return
    }

    terminal.view = null
    if (size !== null) {
      this.resizeTerminal(id, terminal, size)
    }

    const view: View = { connection, attached: false, kept: [] }
    terminal.view = view
    terminal.screen.afterParsed(() => {
      if (terminal.view !== view) {
        return
      }
      const snapshot = terminal.screen.snapshot()
      connection.post({ type: 'attached', id, size: terminal.shownSize, snapshot })
      view.attached = true
      const kept = view.kept
      view.kept = []
      for (const event of kept) {
        connection.post(event)
      }
    })
  }

  private detach(id: SessionId): void {
    const terminal = this.terminals.get(id)
    if (terminal === undefined) {
      return
    }
    terminal.view = null
  }

  private clear(id: SessionId): void {
    const terminal = this.terminals.get(id)
    if (terminal === undefined) {
      return
    }

    terminal.screen.afterParsed(() => {
      terminal.screen.clear()
    })
    this.send(terminal, { type: 'cleared', id })
  }

  private resizeTerminal(id: SessionId, terminal: TerminalState, size: PtySize): void {
    if (sameSize(terminal.size, size)) {
      return
    }

    terminal.size = size
    this.shells.resize(id, size)
    terminal.screen.afterParsed(() => {
      terminal.screen.resize(size)
      terminal.shownSize = size
    })
    this.send(terminal, { type: 'resized', id, size })
  }

  private send(terminal: TerminalState, event: TerminalEvent): void {
    const view = terminal.view
    if (view === null) {
      return
    }
    if (view.attached) {
      view.connection.post(event)
      return
    }
    view.kept.push(event)
  }

  private resumeWhenDrained(id: SessionId, terminal: TerminalState): void {
    if (terminal.unparsed !== 0 || !terminal.paused || terminal.ending) {
      return
    }
    terminal.paused = false
    this.shells.resume(id)
  }
}
