import { unlinkSync, writeFileSync } from 'node:fs'
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import type { SessionId } from '@shared/ipc'
import { claude } from '@shared/harness/claude'
import type { Harness } from '@shared/harness/types'
import type { AgentSignal } from '@shared/rowState'
import { type AgentSession, agentSessionFor } from '@shared/snapshot'

export interface FocusServerHandlers {
  sessionForPids(pids: number[]): SessionId | null
  onFocus(id: SessionId): void
  onAttention(id: SessionId): void
  onAgentStart(id: SessionId, session: AgentSession, hasConversation: boolean): void
  onAgentEnd(id: SessionId, harness: string, sessionId: string): void
  onAgentSignal(id: SessionId, harness: string, sessionId: string, signal: AgentSignal): void
  onUpdateCheck(): void
  onUnavailable(): void
}

const FIRST_PORT = 23456
const LAST_PORT = 23460
// Deliberately distinct: /attention only marks a pane as waiting, while
// /focus-tab switches to it at once. Merged, every ping would steal focus.
const FOCUS_TAB_PATH = '/focus-tab'
const ATTENTION_PATH = '/attention'
const LEGACY_START_PATH = '/claude-session/start'
const LEGACY_END_PATH = '/claude-session/end'
const LEGACY_STATE_PATH = '/claude-session/state'
// Must match scripts/bundle.mjs, or promote cannot reach the running app.
const UPDATE_CHECK_PATH = '/update/check'
// A pid chain is a handful of numbers, so anything larger is not one of ours.
const MAX_BODY_BYTES = 64 * 1024

function writePortFile(portFilePath: string, port: number): void {
  try {
    writeFileSync(portFilePath, String(port))
  } catch {
    return
  }
}

function deletePortFile(portFilePath: string): void {
  try {
    unlinkSync(portFilePath)
  } catch {
    return
  }
}

function attemptListen(
  server: Server,
  port: number,
  onBound: (port: number) => void,
  onUnavailable: () => void
): void {
  function cleanup(): void {
    server.removeListener('error', onError)
    server.removeListener('listening', onListening)
  }

  function onError(error: NodeJS.ErrnoException): void {
    cleanup()
    const nextPort = port + 1
    const outOfPorts = error.code !== 'EADDRINUSE' || nextPort > LAST_PORT
    if (outOfPorts) {
      onUnavailable()
      return
    }
    attemptListen(server, nextPort, onBound, onUnavailable)
  }

  function onListening(): void {
    cleanup()
    onBound(port)
  }

  server.once('error', onError)
  server.once('listening', onListening)
  server.listen(port, '127.0.0.1')
}

function pidsFrom(body: unknown): number[] | null {
  if (typeof body !== 'object' || body === null) {
    return null
  }

  const record = body as Record<string, unknown>
  const candidate = record.pids
  if (!Array.isArray(candidate) || candidate.length === 0) {
    return null
  }

  const finite = candidate.filter((value) => typeof value === 'number' && Number.isFinite(value))
  return finite
}

function sessionIdFrom(harness: Harness, body: unknown): string | null {
  if (typeof body !== 'object' || body === null) {
    return null
  }

  const record = body as Record<string, unknown>
  const candidate = record.sessionId
  if (!harness.isSessionId(candidate)) {
    return null
  }
  return candidate
}

function hasConversationFrom(body: unknown): boolean {
  if (typeof body !== 'object' || body === null) {
    return false
  }

  const record = body as Record<string, unknown>
  return record.hasConversation === true
}

function eventFrom(body: unknown): string | null {
  if (typeof body !== 'object' || body === null) {
    return null
  }

  const record = body as Record<string, unknown>
  const candidate = record.event
  if (typeof candidate !== 'string') {
    return null
  }
  return candidate
}

function detailFrom(body: unknown): string | null {
  if (typeof body !== 'object' || body === null) {
    return null
  }

  const record = body as Record<string, unknown>
  const candidate = record.detail
  if (typeof candidate !== 'string' || candidate === '') {
    return null
  }
  return candidate
}

function readBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    let received = 0
    request.on('data', (chunk: Buffer) => {
      received += chunk.length
      if (received > MAX_BODY_BYTES) {
        reject(new Error('body too large'))
        return
      }
      chunks.push(chunk)
    })
    request.on('end', () => {
      const body = Buffer.concat(chunks).toString('utf8')
      resolve(body)
    })
    request.on('error', reject)
  })
}

function respond(response: ServerResponse, status: number, body: string): void {
  response.writeHead(status)
  response.end(body)
}

async function handleRequest(
  handlers: FocusServerHandlers,
  request: IncomingMessage,
  response: ServerResponse
): Promise<void> {
  // Not tied to a pane, so it carries no pids: any local caller can send it, and
  // all it can do is start a look for a new release.
  if (request.method === 'POST' && request.url === UPDATE_CHECK_PATH) {
    handlers.onUpdateCheck()
    respond(response, 200, 'ok')
    return
  }

  const isFocusTab = request.url === FOCUS_TAB_PATH
  const isAttention = request.url === ATTENTION_PATH
  const isStart = request.url === LEGACY_START_PATH
  const isEnd = request.url === LEGACY_END_PATH
  const isState = request.url === LEGACY_STATE_PATH
  const isKnownRoute = isFocusTab || isAttention || isStart || isEnd || isState
  if (request.method !== 'POST' || !isKnownRoute) {
    respond(response, 404, '')
    return
  }

  let rawBody: string
  try {
    rawBody = await readBody(request)
  } catch {
    respond(response, 413, 'body too large')
    return
  }

  let payload: unknown
  try {
    payload = JSON.parse(rawBody)
  } catch {
    respond(response, 400, 'bad json')
    return
  }

  const pids = pidsFrom(payload)
  if (pids === null) {
    respond(response, 400, 'no pids')
    return
  }

  const harness = claude

  let startedSession: AgentSession | null = null
  if (isStart) {
    startedSession = agentSessionFor(harness, payload)
    if (startedSession === null) {
      respond(response, 400, 'bad session')
      return
    }
  }

  let endedSessionId: string | null = null
  if (isEnd) {
    endedSessionId = sessionIdFrom(harness, payload)
    if (endedSessionId === null) {
      respond(response, 400, 'bad session')
      return
    }
  }

  let signalSessionId: string | null = null
  let stateSignal: AgentSignal | null = null
  if (isState) {
    signalSessionId = sessionIdFrom(harness, payload)
    if (signalSessionId === null) {
      respond(response, 400, 'bad session')
      return
    }
    const event = eventFrom(payload)
    if (event === null) {
      respond(response, 400, 'bad event')
      return
    }
    const detail = detailFrom(payload)
    stateSignal = harness.signalFor(event, detail)
  }

  const paneId = handlers.sessionForPids(pids)
  if (paneId === null) {
    respond(response, 404, 'not found')
    return
  }

  if (isFocusTab) {
    handlers.onFocus(paneId)
  } else if (isAttention) {
    handlers.onAttention(paneId)
  } else if (startedSession !== null) {
    const hasConversation = hasConversationFrom(payload)
    handlers.onAgentStart(paneId, startedSession, hasConversation)
  } else if (endedSessionId !== null) {
    handlers.onAgentEnd(paneId, harness.id, endedSessionId)
  } else if (signalSessionId !== null && stateSignal !== null) {
    handlers.onAgentSignal(paneId, harness.id, signalSessionId, stateSignal)
  }
  respond(response, 200, 'ok')
}

export function startFocusServer(handlers: FocusServerHandlers, portFilePath: string): () => void {
  const server = createServer((request, response) => {
    handleRequest(handlers, request, response).catch(() => {
      response.destroy()
    })
  })

  let boundPort: number | null = null
  attemptListen(
    server,
    FIRST_PORT,
    (port) => {
      boundPort = port
      writePortFile(portFilePath, port)
    },
    handlers.onUnavailable
  )

  return () => {
    server.close()
    // Every port taken means another instance owns the file, and deleting it
    // would cut that one off from its hook.
    if (boundPort === null) {
      return
    }
    deletePortFile(portFilePath)
  }
}
