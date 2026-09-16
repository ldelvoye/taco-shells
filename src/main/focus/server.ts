import { unlinkSync, writeFileSync } from 'node:fs'
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import type { SessionId } from '@shared/ipc'

export interface FocusServerHandlers {
  sessionForPids(pids: number[]): SessionId | null
  onFocus(id: SessionId): void
  onAttention(id: SessionId): void
  onUnavailable(): void
}

const FIRST_PORT = 23456
const LAST_PORT = 23460
// Deliberately distinct: /attention only marks a pane as waiting, while
// /focus-tab switches to it at once. Merged, every ping would steal focus.
const FOCUS_TAB_PATH = '/focus-tab'
const ATTENTION_PATH = '/attention'
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
  const isFocusTab = request.url === FOCUS_TAB_PATH
  const isAttention = request.url === ATTENTION_PATH
  const isKnownRoute = isFocusTab || isAttention
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

  const sessionId = handlers.sessionForPids(pids)
  if (sessionId === null) {
    respond(response, 404, 'not found')
    return
  }

  if (isFocusTab) {
    handlers.onFocus(sessionId)
  } else {
    handlers.onAttention(sessionId)
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
