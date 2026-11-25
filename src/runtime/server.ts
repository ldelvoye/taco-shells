import type { RuntimeCommand, RuntimeQuery, RuntimeReport, TerminalInput, TerminalOutput } from '@protocol/runtimeMessages'
import type { PtySessions } from './sessions'

type Connection = (output: TerminalOutput) => void

export interface RuntimeServer {
  receive(command: RuntimeCommand): void
  /** The window's connection: its input, and where output goes. A second call replaces the first. */
  connectWindow(post: (output: TerminalOutput) => void): (input: TerminalInput) => void
}

export function serveRuntime(sessions: PtySessions, post: (report: RuntimeReport) => void): RuntimeServer {
  // The shells start before the page has loaded, and a message to a page that is
  // not listening yet is dropped: the first prompt would be lost.
  let held: TerminalOutput[] | null = []
  let current: Connection | null = null

  function deliver(output: TerminalOutput): void {
    if (held !== null) {
      held.push(output)
      return
    }
    if (current !== null) {
      current(output)
    }
  }

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
    deliver({ type: 'data', event })
  })

  sessions.onExit((event) => {
    deliver({ type: 'exit', event })
    post({ type: 'exited', event })
  })

  return {
    receive(command: RuntimeCommand): void {
      if (command.type === 'open') {
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
        sessions.resize(command.id, command.size)
        return
      }
      const number = command.ask
      void answer(command.query).then((value) => {
        post({ type: 'answer', ask: number, value })
      })
    },
    connectWindow(connection: Connection): (input: TerminalInput) => void {
      current = connection
      if (held === null) {
        held = []
      }

      return (input) => {
        if (input.type === 'write') {
          sessions.write(input.id, input.data)
          return
        }
        if (input.type === 'resize') {
          sessions.resize(input.id, input.size)
          return
        }
        if (current !== connection) {
          return
        }
        const backlog = held
        held = null
        if (backlog === null) {
          return
        }
        for (const output of backlog) {
          connection(output)
        }
      }
    }
  }
}
