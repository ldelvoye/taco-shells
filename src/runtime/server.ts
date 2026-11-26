import type {
  RuntimeCommand,
  RuntimeQuery,
  RuntimeReport,
  TerminalEvent,
  TerminalRequest
} from '@protocol/runtimeMessages'
import type { Screens } from './screens'
import type { PtySessions } from './sessions'

export interface RuntimeServer {
  receive(command: RuntimeCommand): void
  /** The window's connection: its requests, and where its events go. A second call replaces the first. */
  connectWindow(post: (event: TerminalEvent) => void): (request: TerminalRequest) => void
}

export function serveRuntime(
  sessions: PtySessions,
  screens: Screens,
  post: (report: RuntimeReport) => void
): RuntimeServer {
  async function answer(query: RuntimeQuery): Promise<string | string[] | null> {
    if (query.kind === 'directoryOf') {
      return sessions.directoryOf(query.id)
    }
    if (query.kind === 'foregroundCommands') {
      return sessions.foregroundCommands(query.id)
    }
    return sessions.terminalForPids(query.pids)
  }

  sessions.onStarted((event) => {
    post({ type: 'started', event })
  })

  sessions.onSubmit((id) => {
    post({ type: 'submitted', id })
  })

  sessions.onData((event) => {
    screens.output(event.id, event.data)
  })

  sessions.onExit((event) => {
    screens.ended(event.id, event.exitCode)
    post({ type: 'exited', event })
  })

  screens.onTitle((id, title) => {
    post({ type: 'titled', id, title })
  })

  return {
    receive(command: RuntimeCommand): void {
      if (command.type === 'open') {
        screens.open(command.start.id, command.start.size)
        sessions.open(command.start)
        return
      }
      if (command.type === 'close') {
        sessions.close(command.id)
        return
      }
      if (command.type === 'write') {
        sessions.write(command.id, command.data)
        return
      }
      if (command.type === 'resize') {
        screens.resize(command.id, command.size)
        return
      }
      if (command.type === 'scrollback') {
        screens.setScrollback(command.lines)
        return
      }
      if (command.type === 'colors') {
        screens.setColors(command.colors)
        return
      }
      const number = command.ask
      void answer(command.query).then((value) => {
        post({ type: 'answer', ask: number, value })
      })
    },
    connectWindow(connection: (event: TerminalEvent) => void): (request: TerminalRequest) => void {
      return screens.connect(connection)
    }
  }
}
