import { existsSync, mkdtempSync, readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import type { SessionId } from '@shared/ipc'
import type { ClaudeSession } from '@shared/snapshot'
import { type FocusServerHandlers, startFocusServer } from './server'

interface FakeHandlers extends FocusServerHandlers {
  sessionPids: Map<SessionId, number>
  focused: SessionId[]
  attended: SessionId[]
  started: Array<[SessionId, ClaudeSession]>
  ended: Array<[SessionId, string]>
  unavailable: boolean[]
}

function createFakeHandlers(): FakeHandlers {
  const sessionPids = new Map<SessionId, number>()
  const focused: SessionId[] = []
  const attended: SessionId[] = []
  const started: Array<[SessionId, ClaudeSession]> = []
  const ended: Array<[SessionId, string]> = []
  const unavailable: boolean[] = []

  return {
    sessionPids,
    focused,
    attended,
    started,
    ended,
    unavailable,
    onClaudeStart: (id: SessionId, session: ClaudeSession) => {
      started.push([id, session])
    },
    onClaudeEnd: (id: SessionId, sessionId: string) => {
      ended.push([id, sessionId])
    },
    onUnavailable: () => {
      unavailable.push(true)
    },
    sessionForPids: (pids: number[]) => {
      for (const [id, pid] of sessionPids) {
        if (pids.includes(pid)) {
          return id
        }
      }
      return null
    },
    onFocus: (id: SessionId) => {
      focused.push(id)
    },
    onAttention: (id: SessionId) => {
      attended.push(id)
    }
  }
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}

async function waitForPortFile(portFilePath: string): Promise<number> {
  const deadline = Date.now() + 2000
  while (Date.now() < deadline) {
    if (existsSync(portFilePath)) {
      const text = readFileSync(portFilePath, 'utf8')
      return Number(text)
    }
    await sleep(10)
  }
  throw new Error(`timed out waiting for ${portFilePath}`)
}

function post(port: number, path: string, body: unknown): Promise<Response> {
  return fetch(`http://127.0.0.1:${port}${path}`, {
    method: 'POST',
    body: JSON.stringify(body)
  })
}

function freshPortFilePath(): string {
  const directory = mkdtempSync(join(tmpdir(), 'taco-shells-focus-test-'))
  return join(directory, 'focus-port')
}

describe('startFocusServer', () => {
  let handlers: FakeHandlers
  let stop: () => void
  let port: number

  beforeEach(async () => {
    handlers = createFakeHandlers()
    const portFilePath = freshPortFilePath()
    stop = startFocusServer(handlers, portFilePath)
    port = await waitForPortFile(portFilePath)
  })

  afterEach(() => {
    stop()
  })

  it('resolves a session from a pid in the middle of the chain', async () => {
    handlers.sessionPids.set('pty-2', 222)

    const response = await post(port, '/focus-tab', { pids: [999, 222, 1] })

    expect(response.status).toBe(200)
    expect(handlers.focused).toEqual(['pty-2'])
  })

  it('answers 404 and calls neither handler when no pid in the chain matches', async () => {
    handlers.sessionPids.set('pty-2', 222)

    const response = await post(port, '/attention', { pids: [999, 888] })

    expect(response.status).toBe(404)
    expect(handlers.focused).toEqual([])
    expect(handlers.attended).toEqual([])
  })

  it('keeps /attention and /focus-tab distinct', async () => {
    handlers.sessionPids.set('pty-2', 222)

    await post(port, '/attention', { pids: [222] })
    expect(handlers.attended).toEqual(['pty-2'])
    expect(handlers.focused).toEqual([])

    await post(port, '/focus-tab', { pids: [222] })
    expect(handlers.focused).toEqual(['pty-2'])
    expect(handlers.attended).toEqual(['pty-2'])
  })
})

describe('startFocusServer Claude session routes', () => {
  const SESSION_ID = '123e4567-e89b-12d3-a456-426614174000'
  let handlers: FakeHandlers
  let stop: () => void
  let port: number

  beforeEach(async () => {
    handlers = createFakeHandlers()
    handlers.sessionPids.set('pty-2', 222)
    const portFilePath = freshPortFilePath()
    stop = startFocusServer(handlers, portFilePath)
    port = await waitForPortFile(portFilePath)
  })

  afterEach(() => {
    stop()
  })

  it('records a valid start once', async () => {
    const body = { pids: [999, 222], sessionId: SESSION_ID, cwd: '/tmp/work' }

    const response = await post(port, '/claude-session/start', body)

    expect(response.status).toBe(200)
    expect(handlers.started).toEqual([['pty-2', { sessionId: SESSION_ID, cwd: '/tmp/work' }]])
  })

  it('answers 400 and records nothing for a malformed session', async () => {
    const badId = await post(port, '/claude-session/start', { pids: [222], sessionId: 'abc', cwd: '/tmp/work' })
    const noCwd = await post(port, '/claude-session/start', { pids: [222], sessionId: SESSION_ID })
    const badEndId = await post(port, '/claude-session/end', { pids: [222], sessionId: 'abc' })

    expect([badId.status, noCwd.status, badEndId.status]).toEqual([400, 400, 400])
    expect(handlers.started).toEqual([])
    expect(handlers.ended).toEqual([])
  })

  it('records a valid end', async () => {
    const body = { pids: [222], sessionId: SESSION_ID }

    const response = await post(port, '/claude-session/end', body)

    expect(response.status).toBe(200)
    expect(handlers.ended).toEqual([['pty-2', SESSION_ID]])
  })

})

function bindBlocker(port: number): Promise<Server> {
  return new Promise((resolve, reject) => {
    const candidate = createServer()
    candidate.once('error', reject)
    candidate.listen(port, '127.0.0.1', () => {
      candidate.removeListener('error', reject)
      resolve(candidate)
    })
  })
}

describe('startFocusServer port selection', () => {
  it('says so rather than failing silently when every port is taken', async () => {
    const blockers: Server[] = []
    for (let port = 23456; port <= 23460; port += 1) {
      try {
        const blocker = await bindBlocker(port)
        blockers.push(blocker)
      } catch {
        // Already held by something else on this machine, which blocks it just as well.
      }
    }

    const handlers = createFakeHandlers()
    const portFilePath = freshPortFilePath()
    const stop = startFocusServer(handlers, portFilePath)
    try {
      await sleep(300)
      expect(handlers.unavailable).toEqual([true])
      expect(existsSync(portFilePath)).toBe(false)
    } finally {
      stop()
      for (const blocker of blockers) {
        await new Promise<void>((resolve) => {
          blocker.close(() => resolve())
        })
      }
    }
  })

  it('falls through to the next port when the first is already bound', async () => {
    const probePortFilePath = freshPortFilePath()
    const probeStop = startFocusServer(createFakeHandlers(), probePortFilePath)
    const firstAvailablePort = await waitForPortFile(probePortFilePath)
    probeStop()

    const blocker = await bindBlocker(firstAvailablePort)
    try {
      const portFilePath = freshPortFilePath()
      const stop = startFocusServer(createFakeHandlers(), portFilePath)
      try {
        const port = await waitForPortFile(portFilePath)
        // Another running Taco Shells may hold the ports in between, so only
        // the direction is certain.
        expect(port).toBeGreaterThan(firstAvailablePort)
      } finally {
        stop()
      }
    } finally {
      await new Promise<void>((resolve) => {
        blocker.close(() => resolve())
      })
    }
  })
})
