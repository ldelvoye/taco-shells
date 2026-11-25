import { describe, expect, it } from 'vitest'

import type { PtyExitEvent } from '@protocol/ipc'
import type { TerminalStart, TerminalStarted } from '@protocol/runtime'
import type { RuntimeCommand, RuntimeReport } from '@protocol/runtimeMessages'
import { createRuntimeProxy, type RuntimeProxy } from './proxy'

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
    proxy.close('t1')

    expect(posted).toEqual([
      { type: 'open', start: START },
      { type: 'write', id: 't1', data: 'ls' },
      { type: 'resize', id: 't1', size: { cols: 100, rows: 30 } },
      { type: 'close', id: 't1' }
    ])
  })

  it('turns a started shell, a submitted line and an exit into events', () => {
    const { proxy } = recordingProxy()
    const started: TerminalStarted[] = []
    const submitted: string[] = []
    const exits: PtyExitEvent[] = []
    proxy.onStarted((event) => started.push(event))
    proxy.onSubmit((id) => submitted.push(id))
    proxy.onExit((event) => exits.push(event))

    feed(proxy, { type: 'started', event: { id: 't1', directory: '/tmp' } })
    feed(proxy, { type: 'submitted', id: 't1' })
    feed(proxy, { type: 'exited', event: { id: 't1', exitCode: 3 } })

    expect(started).toEqual([{ id: 't1', directory: '/tmp' }])
    expect(submitted).toEqual(['t1'])
    expect(exits).toEqual([{ id: 't1', exitCode: 3 }])
  })

  it('answers each query to the caller that asked it', async () => {
    const { proxy, posted } = recordingProxy()

    const first = proxy.directoryOf('t1')
    const second = proxy.directoryOf('t2')
    const [firstAsk, secondAsk] = askNumbers(posted)
    feed(proxy, { type: 'answer', ask: secondAsk, value: '/two' })
    feed(proxy, { type: 'answer', ask: firstAsk, value: '/one' })

    expect(posted.map((command) => command.type)).toEqual(['ask', 'ask'])
    expect(await first).toBe('/one')
    expect(await second).toBe('/two')
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
