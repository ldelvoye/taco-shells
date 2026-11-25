import { type ChildProcess, execFileSync, spawn } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'

import { shellPath } from '../src/main/pty/shell.ts'
import {
  DEBUG_PORT,
  evaluate,
  evaluateInMain,
  findDebuggerUrl,
  INSPECT_PORT,
  openProtocol,
  type Protocol,
  type ProtocolParams
} from './harness/connection.ts'
import { type Key, type Modifiers, press } from './harness/keys.ts'
import { clickColorDot, rightClickRow, SidebarDrags } from './harness/mouse.ts'
import { POLL_INTERVAL_MS, sleep } from './harness/poll.ts'
import { readScreen, type Screen, until } from './harness/screen.ts'

// Asked of the app rather than assumed, because the shell it opens is whichever
// one the machine gives its user. A hardcoded `zsh -l` counts nothing on a
// machine whose login shell is something else.
const LOGIN_SHELL = `${basename(shellPath())} -l`

const REPO_ROOT = resolve(import.meta.dirname, '..')
const ELECTRON_BINARY = resolve(
  REPO_ROOT,
  'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'
)

const SHUTDOWN_TIMEOUT_MS = 3000

const NO_CLAUDE_DIRECTORY = 'no-claude'

// xterm warns when a render task overruns its idle deadline, which a slow CI
// runner does on its own; it says nothing about the app.
const XTERM_DEADLINE_WARNING = 'task queue exceeded allotted deadline'

/** A content size for a window shown on screen, without focus, on the display with the highest scale factor. */
export interface WindowPlacement {
  width: number
  height: number
}

/** What a launch may override: the config directory, anything the shells inherit, whether the window is shown, and the main-process inspector. */
export interface LaunchOptions {
  configDir?: string
  env?: Record<string, string>
  expectedRows?: number
  window?: WindowPlacement
  mainInspector?: boolean
}

export interface App {
  page: Protocol
  drags: SidebarDrags
  cdp(method: string, params?: ProtocolParams): Promise<ProtocolParams>
  onCdpEvent: Protocol['onEvent']
  screen(): Promise<Screen>
  until(description: string, holds: (screen: Screen) => boolean): Promise<Screen>
  press(key: Key, modifiers?: Modifiers): Promise<void>
  rightClickRow(rowIndex: number): Promise<void>
  clickColorDot(color: string): Promise<void>
  runInPane(sessionId: string, command: string): Promise<void>
  pause(milliseconds: number): Promise<void>
  shellCount(): number
  rendererConsole(): string[]
  close(): Promise<void>
  quitGracefully(): Promise<void>
  waitForExit(): Promise<void>
}

function whenExited(child: ChildProcess): Promise<void> {
  const stopped = child.exitCode !== null || child.signalCode !== null
  if (stopped) {
    return Promise.resolve()
  }
  return new Promise((exited) => {
    child.once('exit', () => exited())
  })
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
  const script = placementScript(placement)
  await evaluateInMain(script)
}

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
  // The same goes for Claude Code's settings: the app is pointed at a directory
  // that does not exist unless a test passes its own.
  env.CLAUDE_CONFIG_DIR = join(env.TACO_SHELLS_CONFIG_DIR, NO_CLAUDE_DIRECTORY)
  if (options.env) {
    Object.assign(env, options.env)
  }

  const launchArguments = ['.', `--remote-debugging-port=${DEBUG_PORT}`]
  if (options.window || options.mainInspector) {
    launchArguments.push(`--inspect=${INSPECT_PORT}`)
  }
  if (options.window) {
    // The window is shown behind whatever the user is doing, and Chromium
    // stops painting an occluded window unless told not to.
    launchArguments.push('--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding')
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

  // An app left running after a failed launch keeps the debug port, and every later launch would attach to it.
  let page: Protocol
  try {
    const debuggerUrl = await findDebuggerUrl(DEBUG_PORT, 'page')
    page = await openProtocol(debuggerUrl)
  } catch (failure) {
    electron.kill('SIGKILL')
    await whenExited(electron)
    discardOwnedConfigDir()
    throw failure
  }
  const send = page.send

  const drags = new SidebarDrags(page)

  async function cdp(method: string, params: ProtocolParams = {}): Promise<ProtocolParams> {
    return send(method, params)
  }

  async function runInPane(sessionId: string, command: string): Promise<void> {
    const keystrokes = JSON.stringify(`${command}\r`)
    const target = JSON.stringify(sessionId)
    await evaluate(page, `window.tacoShells.pty.write(${target}, ${keystrokes})`)
  }

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

  // Chromium tags renderer console output INFO:CONSOLE whatever severity it was
  // logged at, while its own subsystems name a source file instead. Matching the
  // tag keeps this to the app's own output, and the app only logs on failure.
  function rendererConsole(): string[] {
    const lines = output.join('').split('\n')
    const consoleLines = lines.filter((line) => line.includes(':INFO:CONSOLE'))
    return consoleLines.filter((line) => !line.includes(XTERM_DEADLINE_WARNING))
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
    await whenExited(electron)
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
      await Promise.race([evaluate(page, 'window.close()'), socketClosed])
    } catch {
      return waitForExit()
    }
    await waitForExit()
  }

  const app: App = {
    page,
    drags,
    cdp,
    onCdpEvent: page.onEvent,
    screen: () => readScreen(page),
    until: (description, holds) => until(page, description, holds),
    press: (key, modifiers) => press(page, key, modifiers),
    rightClickRow: (rowIndex) => rightClickRow(page, rowIndex),
    clickColorDot: (color) => clickColorDot(page, color),
    runInPane,
    pause,
    shellCount,
    rendererConsole,
    close,
    quitGracefully,
    waitForExit
  }
  let expectedRows = 1
  if (options.expectedRows !== undefined) {
    expectedRows = options.expectedRows
  }
  const waitDescription = `the app shows ${expectedRows} terminal rows`
  try {
    await send('Runtime.enable', {})
    if (options.window) {
      await placeWindow(options.window)
    }
    await until(page, waitDescription, (state) => state.rows.length === expectedRows)
  } catch (failure) {
    await close().catch(() => undefined)
    throw failure
  }

  return app
}
