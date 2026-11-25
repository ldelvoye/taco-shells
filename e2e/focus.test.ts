import { existsSync, mkdtempSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { type App, launchApp } from './app'
import { CHANNEL } from '../src/protocol/ipc.ts'
import { evaluateInMain } from './harness/connection'
import { KEY } from './harness/keys'

const TYPED_KEY = { key: 'x', code: 'KeyX', text: 'x', windowsVirtualKeyCode: 88, nativeVirtualKeyCode: 88 }
const ANSWER_DELAY_MS = 400

// Holds back every state the workspace sends the window, as a busy main process
// would. Input is left alone: blocking the main process itself would hold the
// test's own key presses back too, and nothing would be learned.
const DELAY_WORKSPACE_ANSWERS = `(() => {
  const { BrowserWindow } = process.mainModule.require('electron')
  const contents = BrowserWindow.getAllWindows()[0].webContents
  const send = contents.send.bind(contents)
  contents.send = (channel, ...payload) => {
    if (channel !== ${JSON.stringify(CHANNEL.workspaceState)}) {
      send(channel, ...payload)
      return
    }
    setTimeout(() => {
      send(channel, ...payload)
    }, ${ANSWER_DELAY_MS})
  }
  return ''
})()`

// The window moves the keyboard itself and tells the workspace afterwards. Only a
// real window shows whether that holds while the workspace's answer is late.

interface Witness {
  ready: string
  got: string
}

function witnessIn(directory: string, pane: number): Witness {
  return { ready: join(directory, `ready-${pane}`), got: join(directory, `got-${pane}`) }
}

// Each shell is the only witness to what reached its terminal: it reads one line
// and writes it to a file of its own.
async function listenIn(app: App, pane: number, witness: Witness): Promise<void> {
  const script = `printf ready > ${witness.ready}; read -r line; printf '%s' "$line" > ${witness.got}`
  await app.runInPane(`pty-${pane}`, script)
  await app.until(`terminal ${pane} is reading`, () => existsSync(witness.ready))
}

// A printable key goes in as a key press with its text, the way a keyboard sends it.
async function typeLine(app: App): Promise<void> {
  await app.cdp('Input.dispatchKeyEvent', { ...TYPED_KEY, type: 'keyDown' })
  await app.cdp('Input.dispatchKeyEvent', { ...TYPED_KEY, type: 'keyUp' })
  await app.press(KEY.enter)
}

let app: App

beforeEach(async () => {
  app = await launchApp({ mainInspector: true })
})

afterEach(async () => {
  await app.close()
})

describe('keyboard focus', () => {
  it('a key typed straight after a focus chord lands in the new terminal, before the workspace has answered', async () => {
    const directory = mkdtempSync(join(tmpdir(), 'taco-shells-focus-'))
    await app.press(KEY.t, { cmd: true })
    await app.until('the second terminal is open', (screen) => screen.rows.length === 2)
    const first = witnessIn(directory, 1)
    const second = witnessIn(directory, 2)
    await listenIn(app, 1, first)
    await listenIn(app, 2, second)

    await evaluateInMain(DELAY_WORKSPACE_ANSWERS)
    await app.press(KEY.bracketLeft, { cmd: true, shift: true })
    await typeLine(app)

    await app.until('a terminal got the line', () => existsSync(first.got) || existsSync(second.got))
    expect(existsSync(second.got)).toBe(false)
    expect(readFileSync(first.got, 'utf8')).toBe('x')
  })
})
