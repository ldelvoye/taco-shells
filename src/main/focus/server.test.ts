import { existsSync, mkdtempSync, readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import type { SessionId } from '@shared/ipc'
import { type FocusServerHandlers, startFocusServer } from './server'

interface FakeHandlers extends FocusServerHandlers {
  sessionPids: Map<SessionId, number>
  focused: SessionId[]
  attended: SessionId[]
  unavailable: boolean[]
}

function createFakeHandlers(): FakeHandlers {
  const sessionPids = new Map<SessionId, number>()
  const focused: SessionId[] = []
  const attended: SessionId[] = []
  const unavailable: boolean[] = []

  return {
    sessionPids,
    focused,
    attended,
    unavailable,
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
        expect(port).toBe(firstAvailablePort + 1)
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
