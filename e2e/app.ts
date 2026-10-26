import { execFileSync, spawn } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'

import { shellPath } from '../src/main/pty/shell.ts'

// Asked of the app rather than assumed, because the shell it opens is whichever
// one the machine gives its user. A hardcoded `zsh -l` counts nothing on a
// machine whose login shell is something else.
const LOGIN_SHELL = `${basename(shellPath())} -l`

const REPO_ROOT = resolve(import.meta.dirname, '..')
const ELECTRON_BINARY = resolve(
  REPO_ROOT,
  'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'
)

const DEBUG_PORT = 9422
const INSPECT_PORT = 9423
const LAUNCH_TIMEOUT_MS = 20000
const SETTLE_TIMEOUT_MS = 15000
const SHUTDOWN_TIMEOUT_MS = 3000
const POLL_INTERVAL_MS = 100

// xterm warns when a render task overruns its idle deadline, which a slow CI
// runner does on its own; it says nothing about the app.
const XTERM_DEADLINE_WARNING = 'task queue exceeded allotted deadline'

// Chromium's modifier bitmask for Input.dispatchKeyEvent.
const MODIFIER_ALT = 1
const MODIFIER_META = 4
const MODIFIER_SHIFT = 8

export interface Key {
  code: string
  text: string
  virtualKeyCode: number
}

export const KEY = {
  b: { code: 'KeyB', text: 'b', virtualKeyCode: 66 },
  t: { code: 'KeyT', text: 't', virtualKeyCode: 84 },
  w: { code: 'KeyW', text: 'w', virtualKeyCode: 87 },
  backslash: { code: 'Backslash', text: '\\', virtualKeyCode: 220 },
  bracketLeft: { code: 'BracketLeft', text: '[', virtualKeyCode: 219 },
  bracketRight: { code: 'BracketRight', text: ']', virtualKeyCode: 221 },
  left: { code: 'ArrowLeft', text: '', virtualKeyCode: 37 },
  right: { code: 'ArrowRight', text: '', virtualKeyCode: 39 },
  comma: { code: 'Comma', text: ',', virtualKeyCode: 188 },
  enter: { code: 'Enter', text: 'Enter', virtualKeyCode: 13 }
} satisfies Record<string, Key>

export interface Modifiers {
  cmd?: boolean
  alt?: boolean
  shift?: boolean
}

/** A content size for a window shown on screen, without focus, on the display with the highest scale factor. */
export interface WindowPlacement {
  width: number
  height: number
}

/** What a launch may override: the config directory, anything the shells inherit, and whether the window is shown. */
export interface LaunchOptions {
  configDir?: string
  env?: Record<string, string>
  expectedRows?: number
  window?: WindowPlacement
}

/** A sidebar row: every pane of every group has one, in sidebar order. */
export interface Row {
  label: string
  connector: string
  active: boolean
}

/** A pane of the group currently on screen, left to right. */
export interface VisiblePane {
  width: number
  pixels: number
}

export interface Screen {
  rows: Row[]
  panes: VisiblePane[]
  groups: number
  dividers: number
  sidebarVisible: boolean
  sidebarWidth: number
  windowWidth: number
  labelsTruncated: boolean
}

export type ProtocolParams = Record<string, unknown>

export interface App {
  cdp(method: string, params?: ProtocolParams): Promise<ProtocolParams>
  onCdpEvent(method: string, listener: (params: ProtocolParams) => void): void
  screen(): Promise<Screen>
  press(key: Key, modifiers?: Modifiers): Promise<void>
  pressAutoRepeat(key: Key, modifiers?: Modifiers): Promise<void>
  dragDivider(dividerIndex: number, byPixels: number): Promise<void>
  dragSidebarDivider(byPixels: number): Promise<void>
  doubleClickSidebarDivider(): Promise<void>
  runInPane(sessionId: string, command: string): Promise<void>
  until(description: string, holds: (screen: Screen) => boolean): Promise<Screen>
  pause(milliseconds: number): Promise<void>
  shellCount(): number
  rendererConsole(): string[]
  chromeBackground(): Promise<string>
  configProblems(): Promise<string[]>
  close(): Promise<void>
  quitGracefully(): Promise<void>
  waitForExit(): Promise<void>
}

interface DebugTarget {
  type: string
  webSocketDebuggerUrl: string
}

interface Protocol {
  send(method: string, params: ProtocolParams): Promise<ProtocolParams>
  onEvent(method: string, listener: (params: ProtocolParams) => void): void
  close(): void
  onClose(listener: () => void): void
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((done) => setTimeout(done, milliseconds))
}

function modifierMask(modifiers: Modifiers): number {
  let mask = 0
  if (modifiers.cmd) {
    mask += MODIFIER_META
  }
  if (modifiers.alt) {
    mask += MODIFIER_ALT
  }
  if (modifiers.shift) {
    mask += MODIFIER_SHIFT
  }
  return mask
}

/**
 * Retries `attempt` until it answers with something other than null, or the
 * timeout runs out. Polling rather than sleeping a guessed interval is what keeps
 * these tests from being a pile of timeouts.
 */
export async function pollFor<T>(
  description: string,
  attempt: () => Promise<T | null>,
  timeoutMs: number
): Promise<T> {
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    const found = await attempt()
    if (found !== null) {
      return found
    }
    await sleep(POLL_INTERVAL_MS)
  }

  throw new Error(`timed out waiting for ${description}`)
}

async function findDebuggerUrl(port: number, targetType: string): Promise<string> {
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

async function openProtocol(url: string): Promise<Protocol> {
  const socket = new WebSocket(url)
  await new Promise((opened) => socket.addEventListener('open', opened))

  let nextMessageId = 1
  const awaitingReply = new Map<number, (result: ProtocolParams) => void>()
  const eventListeners = new Map<string, ((params: ProtocolParams) => void)[]>()

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
    const reply = awaitingReply.get(message.id)
    if (!reply) {
      return
    }
    awaitingReply.delete(message.id)
    reply(message.result)
  })

  function send(method: string, params: ProtocolParams): Promise<ProtocolParams> {
    const id = nextMessageId
    nextMessageId += 1
    socket.send(JSON.stringify({ id, method, params }))
    return new Promise((resolved) => awaitingReply.set(id, resolved))
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

// Runs in the main process, which is the only side that can size and show the
// window. The sharpest display wins so a capture of the window is at full
// device resolution. The inspector's global scope has no require of its own.
function placementScript(placement: WindowPlacement): string {
  return `(() => {
    const { BrowserWindow, screen } = process.mainModule.require('electron')
    const window = BrowserWindow.getAllWindows()[0]
    let display = screen.getPrimaryDisplay()
    for (const candidate of screen.getAllDisplays()) {
      if (candidate.scaleFactor > display.scaleFactor) {
        display = candidate
      }
    }
    const area = display.workArea
    const x = Math.round(area.x + (area.width - ${placement.width}) / 2)
    const y = Math.round(area.y + (area.height - ${placement.height}) / 2)
    window.setPosition(x, y)
    window.setContentSize(${placement.width}, ${placement.height})
    window.showInactive()
  })()`
}

async function placeWindow(placement: WindowPlacement): Promise<void> {
  const main = await openProtocol(await findDebuggerUrl(INSPECT_PORT, 'node'))
  const outcome = await main.send('Runtime.evaluate', { expression: placementScript(placement) })
  main.close()
  if (outcome.exceptionDetails) {
    throw new Error(`placing the window failed: ${JSON.stringify(outcome.exceptionDetails)}`)
  }
}

// The sidebar's rows and the on-screen group's panes are two separate lists:
// rows cover every group, panes only the one not hidden.
const SCREEN_SCRIPT = `JSON.stringify({
  rows: [...document.querySelectorAll('.terminal-row')].map((row) => {
    const connector = row.querySelector('.row-connector')
    let piece = ''
    if (connector) {
      piece = connector.className.replace('row-connector ', '')
    }
    return {
      label: row.querySelector('.row-label').textContent,
      connector: piece,
      active: row.classList.contains('is-active')
    }
  }),
  panes: [...document.querySelectorAll('.pane-group:not(.is-inactive) .terminal-view')].map((view) => ({
    width: Number(view.style.flexGrow),
    pixels: Math.round(view.getBoundingClientRect().width)
  })),
  groups: document.querySelectorAll('.pane-group').length,
  dividers: document.querySelectorAll('.pane-group:not(.is-inactive) .pane-divider').length,
  sidebarVisible: document.querySelector('.sidebar') !== null,
  sidebarWidth: (() => {
    const sidebar = document.querySelector('.sidebar')
    if (!sidebar) {
      return 0
    }
    return Math.round(sidebar.getBoundingClientRect().width)
  })(),
  windowWidth: window.innerWidth,
  // A range rather than the usual scrollWidth against clientWidth: both of those
  // are rounded, so they read equal while the label is a fraction over and still
  // showing an ellipsis.
  labelsTruncated: (() => {
    const labels = [...document.querySelectorAll('.row-label')]
    return labels.some((label) => {
      const range = document.createRange()
      range.selectNodeContents(label)
      const textWidth = range.getBoundingClientRect().width
      const availableWidth = label.getBoundingClientRect().width
      return textWidth > availableWidth
    })
  })()
})`

export async function launchApp(options: LaunchOptions = {}): Promise<App> {
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    ELECTRON_ENABLE_LOGGING: '1',
    TACO_SHELLS_BACKGROUND_WINDOW: '1'
  }
  // A launch with no directory of its own still gets one, so that no test can
  // read the config belonging to whoever is running it.
  let ownedConfigDir: string | null = null
  if (options.configDir) {
    env.TACO_SHELLS_CONFIG_DIR = options.configDir
  } else {
    ownedConfigDir = mkdtempSync(join(tmpdir(), 'taco-shells-e2e-config-'))
    env.TACO_SHELLS_CONFIG_DIR = ownedConfigDir
  }
  if (options.env) {
    Object.assign(env, options.env)
  }

  const launchArguments = ['.', `--remote-debugging-port=${DEBUG_PORT}`]
  if (options.window) {
    // The window is shown behind whatever the user is doing, and Chromium
    // stops painting an occluded window unless told not to.
    launchArguments.push(
      `--inspect=${INSPECT_PORT}`,
      '--disable-backgrounding-occluded-windows',
      '--disable-renderer-backgrounding'
    )
  }

  const electron = spawn(ELECTRON_BINARY, launchArguments, {
    cwd: REPO_ROOT,
    env,
    stdio: ['ignore', 'pipe', 'pipe']
  })

  const output: string[] = []
  if (electron.stdout) {
    electron.stdout.on('data', (chunk) => output.push(String(chunk)))
  }
  if (electron.stderr) {
    electron.stderr.on('data', (chunk) => output.push(String(chunk)))
  }

  const page = await openProtocol(await findDebuggerUrl(DEBUG_PORT, 'page'))
  const send = page.send

  async function cdp(method: string, params: ProtocolParams = {}): Promise<ProtocolParams> {
    return send(method, params)
  }

  async function evaluate(expression: string): Promise<string> {
    const outcome = await send('Runtime.evaluate', {
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

  async function screen(): Promise<Screen> {
    const raw = await evaluate(SCREEN_SCRIPT)
    return JSON.parse(raw) as Screen
  }

  async function until(description: string, holds: (screen: Screen) => boolean): Promise<Screen> {
    const deadline = Date.now() + SETTLE_TIMEOUT_MS
    let latest = await screen()

    while (Date.now() < deadline) {
      if (holds(latest)) {
        return latest
      }
      await sleep(POLL_INTERVAL_MS)
      latest = await screen()
    }

    throw new Error(`timed out waiting until ${description}; last saw ${JSON.stringify(latest)}`)
  }

  // Chromium's own input pipeline rather than a synthetic KeyboardEvent, so a
  // key travels the path a real one does, xterm's textarea included.
  async function dispatchKey(key: Key, modifiers: Modifiers, held: boolean): Promise<void> {
    const stroke = {
      modifiers: modifierMask(modifiers),
      code: key.code,
      key: key.text,
      windowsVirtualKeyCode: key.virtualKeyCode,
      nativeVirtualKeyCode: key.virtualKeyCode,
      autoRepeat: held
    }

    await send('Input.dispatchKeyEvent', { ...stroke, type: 'rawKeyDown' })
    await send('Input.dispatchKeyEvent', { ...stroke, type: 'keyUp' })
  }

  async function press(key: Key, modifiers: Modifiers = {}): Promise<void> {
    await dispatchKey(key, modifiers, false)
  }

  /** A keystroke the keyboard produced only because the key is still down. */
  async function pressAutoRepeat(key: Key, modifiers: Modifiers = {}): Promise<void> {
    await dispatchKey(key, modifiers, true)
  }

  async function centreOf(selector: string, index: number): Promise<{ x: number; y: number }> {
    const centreScript = `JSON.stringify((() => {
      const handles = document.querySelectorAll('${selector}')
      const box = handles[${index}].getBoundingClientRect()
      return { x: Math.round(box.left + box.width / 2), y: Math.round(box.top + box.height / 2) }
    })())`
    const measured = await evaluate(centreScript)
    return JSON.parse(measured) as { x: number; y: number }
  }

  async function dragAcross(selector: string, index: number, byPixels: number): Promise<void> {
    const centre = await centreOf(selector, index)

    const held = { button: 'left', buttons: 1, clickCount: 1 }
    const destinationX = centre.x + byPixels

    await send('Input.dispatchMouseEvent', {
      type: 'mousePressed',
      x: centre.x,
      y: centre.y,
      ...held
    })
    await send('Input.dispatchMouseEvent', {
      type: 'mouseMoved',
      x: destinationX,
      y: centre.y,
      ...held
    })
    await send('Input.dispatchMouseEvent', {
      type: 'mouseReleased',
      x: destinationX,
      y: centre.y,
      ...held
    })
  }

  async function dragDivider(dividerIndex: number, byPixels: number): Promise<void> {
    await dragAcross('.pane-group:not(.is-inactive) .pane-divider', dividerIndex, byPixels)
  }

  async function dragSidebarDivider(byPixels: number): Promise<void> {
    await dragAcross('.sidebar-divider', 0, byPixels)
  }

  async function doubleClickSidebarDivider(): Promise<void> {
    const centre = await centreOf('.sidebar-divider', 0)
    const clicked = { button: 'left', buttons: 1, clickCount: 2 }

    await send('Input.dispatchMouseEvent', {
      type: 'mousePressed',
      x: centre.x,
      y: centre.y,
      ...clicked
    })
    await send('Input.dispatchMouseEvent', {
      type: 'mouseReleased',
      x: centre.x,
      y: centre.y,
      ...clicked
    })
  }

  async function runInPane(sessionId: string, command: string): Promise<void> {
    const keystrokes = JSON.stringify(`${command}\r`)
    const target = JSON.stringify(sessionId)
    await evaluate(`window.tacoShells.pty.write(${target}, ${keystrokes})`)
  }

  // Chromium tags renderer console output INFO:CONSOLE whatever severity it was
  // logged at, while its own subsystems name a source file instead. Matching the
  // tag keeps this to the app's own output, and the app only logs on failure.
  function pause(milliseconds: number): Promise<void> {
    return sleep(milliseconds)
  }

  // The login shells this app has open. They are spawned as direct children of
  // the Electron process, so one that outlives its pane shows up here.
  function shellCount(): number {
    const listing = execFileSync('ps', ['-eo', 'pid,ppid,command'], { encoding: 'utf8' })
    const lines = listing.split('\n')

    const ours = lines.filter((line) => {
      if (!line.includes(LOGIN_SHELL)) {
        return false
      }
      const columns = line.trim().split(/\s+/)
      return columns[1] === String(electron.pid)
    })

    return ours.length
  }

  function rendererConsole(): string[] {
    const lines = output.join('').split('\n')
    const consoleLines = lines.filter((line) => line.includes(':INFO:CONSOLE'))
    return consoleLines.filter((line) => !line.includes(XTERM_DEADLINE_WARNING))
  }

  async function chromeBackground(): Promise<string> {
    return evaluate(
      "getComputedStyle(document.documentElement).getPropertyValue('--chrome-background').trim()"
    )
  }

  async function configProblems(): Promise<string[]> {
    const raw = await evaluate(
      "JSON.stringify([...document.querySelectorAll('.config-problem')].map((el) => el.textContent))"
    )
    return JSON.parse(raw) as string[]
  }

  function discardOwnedConfigDir(): void {
    if (ownedConfigDir === null) {
      return
    }
    rmSync(ownedConfigDir, { recursive: true, force: true })
  }

  async function waitForExit(): Promise<void> {
    const deadline = Date.now() + SHUTDOWN_TIMEOUT_MS
    while (Date.now() < deadline) {
      const stopped = electron.exitCode !== null || electron.signalCode !== null
      if (stopped) {
        page.close()
        discardOwnedConfigDir()
        return
      }
      await sleep(POLL_INTERVAL_MS)
    }

    electron.kill('SIGKILL')
    page.close()
    discardOwnedConfigDir()
  }

  async function close(): Promise<void> {
    // The signal has to reach Electron itself. Launching it through a wrapper
    // like npx and killing that leaves Electron running, still holding the ptys.
    electron.kill('SIGTERM')
    await waitForExit()
  }

  async function quitGracefully(): Promise<void> {
    const socketClosed = new Promise<void>((closed) => {
      page.onClose(() => closed())
    })
    try {
      await Promise.race([evaluate('window.close()'), socketClosed])
    } catch {
      return waitForExit()
    }
    await waitForExit()
  }

  await send('Runtime.enable', {})
  if (options.window) {
    try {
      await placeWindow(options.window)
    } catch (failure) {
      electron.kill('SIGTERM')
      throw failure
    }
  }

  const app: App = {
    cdp,
    onCdpEvent: page.onEvent,
    screen,
    press,
    pressAutoRepeat,
    dragDivider,
    dragSidebarDivider,
    doubleClickSidebarDivider,
    runInPane,
    until,
    pause,
    shellCount,
    rendererConsole,
    chromeBackground,
    configProblems,
    close,
    quitGracefully,
    waitForExit
  }
  let expectedRows = 1
  if (options.expectedRows !== undefined) {
    expectedRows = options.expectedRows
  }
  const waitDescription = `the app shows ${expectedRows} terminal rows`
  await until(waitDescription, (state) => state.rows.length === expectedRows)

  return app
}
