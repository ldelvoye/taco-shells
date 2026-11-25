import type { PtyExitEvent, PtySize, SessionId } from '@protocol/ipc'
import type { Runtime, TerminalStart, TerminalStarted } from '@protocol/runtime'
import type { RuntimeCommand, RuntimeQuery, RuntimeReport } from '@protocol/runtimeMessages'

type Answer = string | string[] | null

export interface RuntimeProxy extends Runtime {
  receive(report: RuntimeReport): void
  /** Answers every query still in flight with nothing, and all later ones at once. */
  disconnect(): void
}

export function createRuntimeProxy(post: (command: RuntimeCommand) => void): RuntimeProxy {
  const startedListeners: ((event: TerminalStarted) => void)[] = []
  const exitListeners: ((event: PtyExitEvent) => void)[] = []
  const submitListeners: ((id: SessionId) => void)[] = []
  const inFlight = new Map<number, (value: Answer) => void>()
  let nextAsk = 1
  let connected = true

  function ask(query: RuntimeQuery, settle: (value: Answer) => void): void {
    if (!connected) {
      settle(null)
      return
    }
    const number = nextAsk
    nextAsk += 1
    inFlight.set(number, settle)
    post({ type: 'ask', ask: number, query })
  }

  function askForText(query: RuntimeQuery): Promise<string | null> {
    return new Promise((resolve) => {
      ask(query, (value) => {
        if (typeof value === 'string') {
          resolve(value)
          return
        }
        resolve(null)
      })
    })
  }

  function askForList(query: RuntimeQuery): Promise<string[]> {
    return new Promise((resolve) => {
      ask(query, (value) => {
        if (Array.isArray(value)) {
          resolve(value)
          return
        }
        resolve([])
      })
    })
  }

  return {
    open(start: TerminalStart): void {
      post({ type: 'open', start })
    },
    close(id: SessionId): void {
      post({ type: 'close', id })
    },
    write(id: SessionId, data: string): void {
      post({ type: 'write', id, data })
    },
    resize(id: SessionId, size: PtySize): void {
      post({ type: 'resize', id, size })
    },
    onStarted(listener): void {
      startedListeners.push(listener)
    },
    onExit(listener): void {
      exitListeners.push(listener)
    },
    onSubmit(listener): void {
      submitListeners.push(listener)
    },
    directoryOf(id: SessionId): Promise<string | null> {
      return askForText({ kind: 'directoryOf', id })
    },
    foregroundCommands(id: SessionId): Promise<string[]> {
      return askForList({ kind: 'foregroundCommands', id })
    },
    terminalForPids(pids: number[]): Promise<SessionId | null> {
      return askForText({ kind: 'terminalForPids', pids })
    },
    receive(report: RuntimeReport): void {
      if (report.type === 'started') {
        for (const listener of startedListeners) {
          listener(report.event)
        }
        return
      }
      if (report.type === 'exited') {
        for (const listener of exitListeners) {
          listener(report.event)
        }
        return
      }
      if (report.type === 'submitted') {
        for (const listener of submitListeners) {
          listener(report.id)
        }
        return
      }
      const settle = inFlight.get(report.ask)
      if (settle === undefined) {
        return
      }
      inFlight.delete(report.ask)
      settle(report.value)
    },
    disconnect(): void {
      connected = false
      const settlers = [...inFlight.values()]
      inFlight.clear()
      for (const settle of settlers) {
        settle(null)
      }
    }
  }
}
