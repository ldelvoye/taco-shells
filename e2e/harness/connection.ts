import { pollFor } from './poll.ts'

export const DEBUG_PORT = 9422
export const INSPECT_PORT = 9423
const LAUNCH_TIMEOUT_MS = 20000

export type ProtocolParams = Record<string, unknown>

interface DebugTarget {
  type: string
  webSocketDebuggerUrl: string
}

export interface Protocol {
  send(method: string, params: ProtocolParams): Promise<ProtocolParams>
  onEvent(method: string, listener: (params: ProtocolParams) => void): void
  close(): void
  onClose(listener: () => void): void
}

interface PendingReply {
  answer: (result: ProtocolParams) => void
  fail: (reason: Error) => void
}

export class PageClosedError extends Error {
  constructor() {
    super('the page closed before it answered')
  }
}

export async function findDebuggerUrl(port: number, targetType: string): Promise<string> {
  return pollFor(
    `the app to expose a debuggable ${targetType}`,
    async () => {
      try {
        const response = await fetch(`http://127.0.0.1:${port}/json/list`)
        const targets = (await response.json()) as DebugTarget[]
        const target = targets.find((candidate) => candidate.type === targetType)
        if (!target) {
          return null
        }
        return target.webSocketDebuggerUrl
      } catch {
        // The devtools endpoint is not listening yet.
        return null
      }
    },
    LAUNCH_TIMEOUT_MS
  )
}

export async function openProtocol(url: string): Promise<Protocol> {
  const socket = new WebSocket(url)
  await new Promise((opened) => socket.addEventListener('open', opened))

  let nextMessageId = 1
  let closed = false
  const awaitingReply = new Map<number, PendingReply>()
  const eventListeners = new Map<string, ((params: ProtocolParams) => void)[]>()

  // A page that has gone never answers, and a request left waiting on it would hang its test.
  socket.addEventListener('close', () => {
    closed = true
    for (const pending of awaitingReply.values()) {
      const reason = new PageClosedError()
      pending.fail(reason)
    }
    awaitingReply.clear()
  })

  socket.addEventListener('message', (event) => {
    const message = JSON.parse(String(event.data))
    if (message.id === undefined) {
      const listeners = eventListeners.get(message.method)
      if (!listeners) {
        return
      }
      for (const listener of listeners) {
        listener(message.params)
      }
      return
    }
    const pending = awaitingReply.get(message.id)
    if (!pending) {
      return
    }
    awaitingReply.delete(message.id)
    pending.answer(message.result)
  })

  function send(method: string, params: ProtocolParams): Promise<ProtocolParams> {
    if (closed) {
      const reason = new PageClosedError()
      return Promise.reject(reason)
    }

    const id = nextMessageId
    nextMessageId += 1
    socket.send(JSON.stringify({ id, method, params }))
    return new Promise((answer, fail) => awaitingReply.set(id, { answer, fail }))
  }

  function onEvent(method: string, listener: (params: ProtocolParams) => void): void {
    const listeners = eventListeners.get(method)
    if (listeners) {
      listeners.push(listener)
      return
    }
    eventListeners.set(method, [listener])
  }

  function close(): void {
    socket.close()
  }

  function onClose(listener: () => void): void {
    socket.addEventListener('close', listener)
  }

  return { send, onEvent, close, onClose }
}

export async function evaluateInMain(expression: string): Promise<string> {
  const url = await findDebuggerUrl(INSPECT_PORT, 'node')
  const main = await openProtocol(url)
  const outcome = await main.send('Runtime.evaluate', { expression, returnByValue: true })
  main.close()
  if (outcome.exceptionDetails) {
    throw new Error(`evaluating in the main process failed: ${JSON.stringify(outcome.exceptionDetails)}`)
  }
  const result = outcome.result as { value?: unknown }
  if (typeof result.value === 'string') {
    return result.value
  }
  return ''
}

export async function evaluate(page: Protocol, expression: string): Promise<string> {
  const outcome = await page.send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true
  })
  if (outcome.exceptionDetails) {
    throw new Error(`evaluating in the app failed: ${JSON.stringify(outcome.exceptionDetails)}`)
  }

  const result = outcome.result as { value: string }
  return result.value
}
