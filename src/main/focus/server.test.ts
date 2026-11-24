import { spawn } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:net'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { claude } from '@shared/harness/claude'
import type { Harness } from '@shared/harness/types'
import type { SessionId } from '@shared/ipc'
import type { AgentSignal } from '@shared/rowState'
import type { AgentSession } from '@shared/snapshot'
import { type FocusServerHandlers, startFocusServer } from './server'

const SESSION_SCRIPT = join(import.meta.dirname, '..', '..', '..', 'hooks', 'taco-shells-claude-session.sh')

interface FakeHandlers extends FocusServerHandlers {
  sessionPids: Map<SessionId, number>
  focused: SessionId[]
  attended: SessionId[]
  started: Array<[SessionId, AgentSession, boolean]>
  ended: Array<[SessionId, string, string]>
  signals: Array<[SessionId, string, string, AgentSignal]>
  updateChecks: boolean[]
  unavailable: boolean[]
}

function createFakeHandlers(): FakeHandlers {
  const sessionPids = new Map<SessionId, number>()
  const focused: SessionId[] = []
  const attended: SessionId[] = []
  const started: Array<[SessionId, AgentSession, boolean]> = []
  const ended: Array<[SessionId, string, string]> = []
  const signals: Array<[SessionId, string, string, AgentSignal]> = []
  const updateChecks: boolean[] = []
  const unavailable: boolean[] = []

  return {
    sessionPids,
    focused,
    attended,
    started,
    ended,
    signals,
    updateChecks,
    unavailable,
    onUpdateCheck: () => {
      updateChecks.push(true)
    },
    onAgentSignal: (id: SessionId, harness: string, sessionId: string, signal: AgentSignal) => {
      signals.push([id, harness, sessionId, signal])
    },
    onAgentStart: (id: SessionId, session: AgentSession, hasConversation: boolean) => {
      started.push([id, session, hasConversation])
    },
    onAgentEnd: (id: SessionId, harness: string, sessionId: string) => {
      ended.push([id, harness, sessionId])
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

// Not spawnSync: the server it posts to runs in this process and has to stay free to answer.
function runSessionScript(configDirectory: string, hookInput: unknown): Promise<void> {
  const env = { TACO_SHELLS_CONFIG_DIR: configDirectory, PATH: '/bin:/usr/bin' }
  const child = spawn('/bin/sh', [SESSION_SCRIPT], { env, stdio: ['pipe', 'ignore', 'ignore'] })
  const input = JSON.stringify(hookInput)
  child.stdin.end(input)
  return new Promise((resolve) => {
    child.on('close', () => {
      resolve()
    })
  })
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

  it('takes an update check from promote, which has no pane and sends no pids', async () => {
    const url = `http://127.0.0.1:${port}/update/check`

    const refused = await fetch(url)
    expect(refused.status).toBe(404)
    expect(handlers.updateChecks).toEqual([])

    const accepted = await fetch(url, { method: 'POST' })
    expect(accepted.status).toBe(200)
    expect(handlers.updateChecks).toEqual([true])
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

describe('startFocusServer agent session routes, for Claude', () => {
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

  it('records a valid start, and whether the session already holds a conversation', async () => {
    const body = { harness: 'claude', pids: [999, 222], sessionId: SESSION_ID, cwd: '/tmp/work' }
    const session = { harness: 'claude', sessionId: SESSION_ID, cwd: '/tmp/work' }

    const response = await post(port, '/agent-session/start', body)
    await post(port, '/agent-session/start', { ...body, hasConversation: true })
    await post(port, '/agent-session/start', { ...body, hasConversation: 'yes' })

    expect(response.status).toBe(200)
    expect(handlers.started).toEqual([
      ['pty-2', session, false],
      ['pty-2', session, true],
      ['pty-2', session, false]
    ])
  })

  it('answers 400 and records nothing for a malformed session', async () => {
    const harness = 'claude'
    const badId = await post(port, '/agent-session/start', {
      harness,
      pids: [222],
      sessionId: 'abc',
      cwd: '/tmp/work'
    })
    const noCwd = await post(port, '/agent-session/start', { harness, pids: [222], sessionId: SESSION_ID })
    const badEndId = await post(port, '/agent-session/end', { harness, pids: [222], sessionId: 'abc' })

    expect([badId.status, noCwd.status, badEndId.status]).toEqual([400, 400, 400])
    expect(handlers.started).toEqual([])
    expect(handlers.ended).toEqual([])
  })

  it('records a valid end', async () => {
    const body = { harness: 'claude', pids: [222], sessionId: SESSION_ID }

    const response = await post(port, '/agent-session/end', body)

    expect(response.status).toBe(200)
    expect(handlers.ended).toEqual([['pty-2', 'claude', SESSION_ID]])
  })

  it('forwards a hook event as its signal', async () => {
    const body = {
      harness: 'claude',
      pids: [222],
      sessionId: SESSION_ID,
      event: 'Notification',
      detail: 'permission_prompt'
    }

    const response = await post(port, '/agent-session/state', body)

    expect(response.status).toBe(200)
    expect(handlers.signals).toEqual([['pty-2', 'claude', SESSION_ID, 'needs']])
  })

  it('drops hook events that carry no signal', async () => {
    const body = {
      harness: 'claude',
      pids: [222],
      sessionId: SESSION_ID,
      event: 'Notification',
      detail: 'idle_prompt'
    }

    const response = await post(port, '/agent-session/state', body)

    expect(response.status).toBe(200)
    expect(handlers.signals).toEqual([])
  })

  it('rejects a state post without a valid session id', async () => {
    const body = { harness: 'claude', pids: [222], sessionId: 'nope', event: 'Stop' }

    const response = await post(port, '/agent-session/state', body)

    expect(response.status).toBe(400)
    expect(handlers.signals).toEqual([])
  })
})

const fake: Harness = {
  id: 'fake',
  name: 'Fake',
  isSessionId: (value: unknown): value is string => value === 'thr_1',
  signalFor: (event: string) => {
    if (event === 'Busy') {
      return 'working'
    }
    return null
  },
  resumeCommand: () => null
}

describe('startFocusServer agent session routes', () => {
  let handlers: FakeHandlers
  let stop: () => void
  let port: number

  beforeEach(async () => {
    handlers = createFakeHandlers()
    handlers.sessionPids.set('pty-2', 222)
    const portFilePath = freshPortFilePath()
    stop = startFocusServer(handlers, portFilePath, [claude, fake])
    port = await waitForPortFile(portFilePath)
  })

  afterEach(() => {
    stop()
  })

  it('takes a session from any registered harness', async () => {
    const start = { pids: [222], harness: 'fake', sessionId: 'thr_1', cwd: '/a' }
    const state = { pids: [222], harness: 'fake', sessionId: 'thr_1', event: 'Busy' }

    const end = { pids: [222], harness: 'fake', sessionId: 'thr_1' }

    const started = await post(port, '/agent-session/start', start)
    const signalled = await post(port, '/agent-session/state', state)
    const ended = await post(port, '/agent-session/end', end)

    expect(started.status).toBe(200)
    expect(signalled.status).toBe(200)
    expect(ended.status).toBe(200)
    expect(handlers.started).toEqual([['pty-2', { harness: 'fake', sessionId: 'thr_1', cwd: '/a' }, false]])
    expect(handlers.signals).toEqual([['pty-2', 'fake', 'thr_1', 'working']])
    expect(handlers.ended).toEqual([['pty-2', 'fake', 'thr_1']])

    // thr_1 is an id only the fake harness accepts
    const fakeIdAsClaude = await post(port, '/agent-session/start', { ...start, harness: 'claude' })
    expect(fakeIdAsClaude.status).toBe(400)
    expect(handlers.started).toHaveLength(1)
  })

  it('refuses a harness it does not know', async () => {
    const base = { pids: [222], sessionId: 'thr_1', cwd: '/a', event: 'Busy' }

    const start = await post(port, '/agent-session/start', { ...base, harness: 'gone' })
    const end = await post(port, '/agent-session/end', { ...base, harness: 'gone' })
    const state = await post(port, '/agent-session/state', { ...base, harness: 'gone' })
    const missing = await post(port, '/agent-session/start', base)

    const responses = [start, end, state, missing]
    const bodies = await Promise.all(responses.map((response) => response.text()))
    expect(responses.map((response) => response.status)).toEqual([400, 400, 400, 400])
    expect(bodies).toEqual(['bad harness', 'bad harness', 'bad harness', 'bad harness'])
    expect(handlers.started).toEqual([])
    expect(handlers.ended).toEqual([])
    expect(handlers.signals).toEqual([])
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

describe("Claude's hook script", () => {
  const SESSION_ID = '123e4567-e89b-12d3-a456-426614174000'
  let handlers: FakeHandlers
  let stop: () => void
  let configDirectory: string

  beforeEach(async () => {
    handlers = createFakeHandlers()
    // The script sends its own ancestors, and the first of them is this process.
    handlers.sessionPids.set('pty-2', process.pid)
    const portFilePath = freshPortFilePath()
    configDirectory = dirname(portFilePath)
    stop = startFocusServer(handlers, portFilePath)
    await waitForPortFile(portFilePath)
  })

  afterEach(() => {
    stop()
  })

  it('has every kind of post it sends accepted', async () => {
    const session = { session_id: SESSION_ID }
    const start = { ...session, hook_event_name: 'SessionStart', cwd: '/tmp/work', source: 'resume' }
    const prompt = { ...session, hook_event_name: 'UserPromptSubmit' }
    const permission = { ...session, hook_event_name: 'Notification', notification_type: 'permission_prompt' }
    const end = { ...session, hook_event_name: 'SessionEnd' }

    await runSessionScript(configDirectory, start)
    await runSessionScript(configDirectory, prompt)
    await runSessionScript(configDirectory, permission)
    await runSessionScript(configDirectory, end)

    const startedSession = { harness: 'claude', sessionId: SESSION_ID, cwd: '/tmp/work' }
    expect(handlers.started).toEqual([['pty-2', startedSession, true]])
    expect(handlers.signals).toEqual([
      ['pty-2', 'claude', SESSION_ID, 'working'],
      ['pty-2', 'claude', SESSION_ID, 'needs']
    ])
    expect(handlers.ended).toEqual([['pty-2', 'claude', SESSION_ID]])
  })
})

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
