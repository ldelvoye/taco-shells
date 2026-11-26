import { describe, expect, it } from 'vitest'

import type { PtyExitEvent } from '@protocol/ipc'
import type { TerminalStart, TerminalStarted } from '@protocol/runtime'
import type { RuntimeCommand, RuntimeReport } from '@protocol/runtimeMessages'
import type { TerminalColors } from '@protocol/theme'
import { createRuntimeProxy, type RuntimeProxy } from './proxy'

const COLORS: TerminalColors = {
  background: '#123456',
  foreground: '#010101',
  cursor: '#020202',
  cursorAccent: '#030303',
  selectionBackground: '#040404',
  black: '#050505',
  red: '#060606',
  green: '#070707',
  yellow: '#080808',
  blue: '#090909',
  magenta: '#0a0a0a',
  cyan: '#0b0b0b',
  white: '#0c0c0c',
  brightBlack: '#0d0d0d',
  brightRed: '#0e0e0e',
  brightGreen: '#0f0f0f',
  brightYellow: '#101010',
  brightBlue: '#111111',
  brightMagenta: '#121212',
  brightCyan: '#131313',
  brightWhite: '#141414'
}

function roundTrip<Message>(message: Message): Message {
  return JSON.parse(JSON.stringify(message))
}

function recordingProxy(): { proxy: RuntimeProxy; posted: RuntimeCommand[] } {
  const posted: RuntimeCommand[] = []
  const proxy = createRuntimeProxy((command) => {
    posted.push(roundTrip(command))
  })
  return { proxy, posted }
}

function feed(proxy: RuntimeProxy, report: RuntimeReport): void {
  proxy.receive(roundTrip(report))
}

function askNumbers(posted: RuntimeCommand[]): number[] {
  const numbers: number[] = []
  for (const command of posted) {
    if (command.type === 'ask') {
      numbers.push(command.ask)
    }
  }
  return numbers
}

const START: TerminalStart = {
  id: 't1',
  directory: '/tmp',
  directoryOf: null,
  command: 'ls',
  size: { cols: 80, rows: 24 }
}

describe('RuntimeProxy', () => {
  it('posts open, write, resize and close as commands', () => {
    const { proxy, posted } = recordingProxy()

    proxy.open(START)
    proxy.write('t1', 'ls')
    proxy.resize('t1', { cols: 100, rows: 30 })
    proxy.setScrollback(500)
    proxy.setColors(COLORS)
    proxy.close('t1')

    expect(posted).toEqual([
      { type: 'open', start: START },
      { type: 'write', id: 't1', data: 'ls' },
      { type: 'resize', id: 't1', size: { cols: 100, rows: 30 } },
      { type: 'scrollback', lines: 500 },
      { type: 'colors', colors: COLORS },
      { type: 'close', id: 't1' }
    ])
  })

  it('turns a started shell, a title, a submitted line and an exit into events', () => {
    const { proxy } = recordingProxy()
    const started: TerminalStarted[] = []
    const titles: string[][] = []
    const submitted: string[] = []
    const exits: PtyExitEvent[] = []
    proxy.onStarted((event) => started.push(event))
    proxy.onTitle((id, title) => titles.push([id, title]))
    proxy.onSubmit((id) => submitted.push(id))
    proxy.onExit((event) => exits.push(event))

    feed(proxy, { type: 'started', event: { id: 't1', directory: '/tmp' } })
    feed(proxy, { type: 'titled', id: 't1', title: 'Tacos' })
    feed(proxy, { type: 'submitted', id: 't1' })
    feed(proxy, { type: 'exited', event: { id: 't1', exitCode: 3 } })

    expect(started).toEqual([{ id: 't1', directory: '/tmp' }])
    expect(titles).toEqual([['t1', 'Tacos']])
    expect(submitted).toEqual(['t1'])
    expect(exits).toEqual([{ id: 't1', exitCode: 3 }])
  })

  it('answers each query to the caller that asked it', async () => {
    const { proxy, posted } = recordingProxy()

    const first = proxy.directoryOf('t1')
    const second = proxy.directoryOf('t2')
    const running = proxy.foregroundCommands('t1')
    const [firstAsk, secondAsk, runningAsk] = askNumbers(posted)
    feed(proxy, { type: 'answer', ask: runningAsk, value: ['vim', 'git'] })
    feed(proxy, { type: 'answer', ask: secondAsk, value: '/two' })
    feed(proxy, { type: 'answer', ask: firstAsk, value: '/one' })

    expect(posted.map((command) => command.type)).toEqual(['ask', 'ask', 'ask'])
    expect(await first).toBe('/one')
    expect(await second).toBe('/two')
    expect(await running).toEqual(['vim', 'git'])
  })

  it('answers with nothing once disconnected', async () => {
    const { proxy, posted } = recordingProxy()

    const inFlight = proxy.directoryOf('t1')
    const [oldAsk] = askNumbers(posted)
    proxy.disconnect()
    const later = await proxy.foregroundCommands('t1')
    feed(proxy, { type: 'answer', ask: oldAsk, value: '/late' })

    expect(await inFlight).toBeNull()
    expect(later).toEqual([])
    expect(await proxy.terminalForPids([1])).toBeNull()
    expect(askNumbers(posted)).toEqual([oldAsk])
  })
})
