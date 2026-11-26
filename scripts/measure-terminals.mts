import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { cpus, tmpdir, totalmem } from 'node:os'
import { join, resolve } from 'node:path'

import { type App, launchApp } from '../e2e/app.ts'
import { DEBUG_PORT, evaluate, evaluateInMain } from '../e2e/harness/connection.ts'
import { type Key, KEY } from '../e2e/harness/keys.ts'
import { pollFor, sleep } from '../e2e/harness/poll.ts'
import { MEMORY, START_LAG, STOP_LAG } from './measure/mainProbe.mts'
import { PAGE_PROBE } from './measure/pageProbe.mts'
import {
  type FloodLoad,
  formatReport,
  type Load,
  type MemoryStage,
  type Parameters,
  type ProcessMemory,
  type Result
} from './measure/report.mts'
import { summarize } from './measure/stats.mts'

const REPO_ROOT = resolve(import.meta.dirname, '..')
const RESULT_DIR = join(REPO_ROOT, 'dist.noindex/measure')

const WINDOW_WIDTH = 1280
const WINDOW_HEIGHT = 800
const TYPING_SPACING_MS = 100
const QUIET_MS = 500
const SCROLLBACK = 100000
const FLOOD_ARGUMENT_LENGTH = 80
const FLOOD_WARMUP_MS = 1000
const MEMORY_SETTLE_MS = 1000
const FRAME_CHECK_MS = 1000
const FRAME_CHECK_MAX_GAP_MS = 40
const FRAME_CHECK_MIN_GAPS = 10
const QUIET_WINDOW_MS = 1000
const QUIET_WINDOW_TRIES = 20
const SWITCH_SETTLE_MS = 100
const FILL_TIMEOUT_MS = 120000
const SWITCH_PAIRS = 10
const KILOBYTES_PER_MEGABYTE = 1024
const BYTES_PER_MEGABYTE = 1024 * 1024
const ETX = '\x03'
const TYPED_KEY = { key: 'x', code: 'KeyX', text: 'x', windowsVirtualKeyCode: 88, nativeVirtualKeyCode: 88 }

const FLAG_DEFAULTS: Record<string, number> = {
  terminals: 24,
  full: 8,
  flooding: 4,
  'flood-seconds': 10,
  samples: 50
}

interface Flags {
  terminals: number
  full: number
  flooding: number
  floodSeconds: number
  samples: number
}

interface Metric {
  type: string
  name?: string
  serviceName?: string
  memory: { workingSetSize: number }
}

interface FrameSample {
  frameGaps: number[]
  longTasks: number
  hidden: boolean
}

function parseFlags(argv: string[]): Flags {
  const values = { ...FLAG_DEFAULTS }
  for (const argument of argv) {
    const parts = /^--([a-z-]+)=(.*)$/.exec(argument)
    if (parts === null) {
      throw new Error(`unrecognised argument ${argument}; flags look like --terminals=24`)
    }
    const name = parts[1]
    const text = parts[2]
    if (!(name in FLAG_DEFAULTS)) {
      throw new Error(`unknown flag --${name}; pick from ${Object.keys(FLAG_DEFAULTS).join(', ')}`)
    }
    const whole = /^\d+$/.test(text)
    const value = Number(text)
    if (!whole || value < 1) {
      throw new Error(`--${name} must be a whole number of at least 1, got "${text}"`)
    }
    values[name] = value
  }

  const flags: Flags = {
    terminals: values.terminals,
    full: values.full,
    flooding: values.flooding,
    floodSeconds: values['flood-seconds'],
    samples: values.samples
  }
  if (flags.flooding > flags.full) {
    throw new Error(`--flooding (${flags.flooding}) must not be more than --full (${flags.full})`)
  }
  if (flags.full + 2 > flags.terminals) {
    throw new Error(`--full (${flags.full}) plus 2 must not be more than --terminals (${flags.terminals})`)
  }
  return flags
}

async function refuseIfPortTaken(): Promise<void> {
  let answered = false
  try {
    await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/version`)
    answered = true
  } catch {
    answered = false
  }
  if (answered) {
    throw new Error(
      `something is already listening on port ${DEBUG_PORT}: quit it, or wait for the e2e run to finish`
    )
  }
}

async function stage<T>(name: string, work: () => Promise<T>): Promise<T> {
  console.error(name)
  try {
    return await work()
  } catch (failure) {
    let message = String(failure)
    if (failure instanceof Error) {
      message = failure.message
    }
    throw new Error(`stage "${name}" failed: ${message}`)
  }
}

async function inPage<T>(app: App, call: string): Promise<T> {
  const expression = `(async () => JSON.stringify(await ${call}))()`
  const raw = await evaluate(app.page, expression)
  return JSON.parse(raw)
}

function terminalId(number: number): string {
  return `pty-${number}`
}

function idList(ids: string[]): string {
  return JSON.stringify(ids)
}

async function memorySnapshot(app: App, stageName: string): Promise<MemoryStage> {
  await app.cdp('HeapProfiler.collectGarbage')
  await sleep(MEMORY_SETTLE_MS)
  const listing = await evaluateInMain(MEMORY)
  const metrics: Metric[] = JSON.parse(listing)
  const heapText = await evaluate(app.page, 'performance.memory.usedJSHeapSize')
  const heapBytes = Number(heapText)

  const seen = new Map<string, number>()
  const processes: ProcessMemory[] = []
  let totalMegabytes = 0
  for (const metric of metrics) {
    let kind = metric.type
    if (metric.serviceName) {
      kind = metric.serviceName
    }
    if (metric.name) {
      kind = metric.name
    }
    const earlier = seen.get(kind)
    let label = kind
    if (earlier === undefined) {
      seen.set(kind, 1)
    } else {
      const ordinal = earlier + 1
      seen.set(kind, ordinal)
      label = `${kind} ${ordinal}`
    }
    const megabytes = metric.memory.workingSetSize / KILOBYTES_PER_MEGABYTE
    processes.push({ kind: label, megabytes })
    totalMegabytes += megabytes
  }
  const windowHeapMegabytes = heapBytes / BYTES_PER_MEGABYTE
  return { stage: stageName, processes, totalMegabytes, windowHeapMegabytes }
}

async function beginLoad(app: App): Promise<void> {
  await evaluateInMain(START_LAG)
  await evaluate(app.page, 'window.__measure.startLoad()')
}

async function endLoad(app: App, typing: number[]): Promise<Load> {
  const sample = await inPage<FrameSample>(app, 'window.__measure.stopLoad()')
  const lagText = await evaluateInMain(STOP_LAG)
  const lag: number[] = JSON.parse(lagText)
  if (sample.hidden) {
    throw new Error('the window was hidden while this was measured, so its numbers would mean nothing: run it again')
  }
  return {
    typingMs: summarize(typing),
    mainLagMs: summarize(lag),
    frameGapMs: summarize(sample.frameGaps),
    longTasks: sample.longTasks
  }
}

async function typeOnce(app: App, id: string): Promise<number> {
  await evaluate(app.page, `window.__measure.armKey(${JSON.stringify(id)})`)
  await app.cdp('Input.dispatchKeyEvent', { ...TYPED_KEY, type: 'keyDown' })
  await app.cdp('Input.dispatchKeyEvent', { ...TYPED_KEY, type: 'keyUp' })
  const elapsed = await inPage<number>(app, 'window.__measure.keyed')
  await sleep(TYPING_SPACING_MS)
  return elapsed
}

async function typeTimes(app: App, id: string, count: number): Promise<number[]> {
  const elapsed: number[] = []
  for (let sample = 0; sample < count; sample += 1) {
    const took = await typeOnce(app, id)
    elapsed.push(took)
  }
  return elapsed
}

async function typeUntil(app: App, id: string, deadline: number): Promise<number[]> {
  const elapsed: number[] = []
  while (Date.now() < deadline) {
    const took = await typeOnce(app, id)
    elapsed.push(took)
  }
  return elapsed
}

async function receivedCharacters(app: App, ids: string[]): Promise<number> {
  const bytes = await inPage<Record<string, number>>(app, 'window.__measure.bytes')
  let total = 0
  for (const id of ids) {
    // A terminal the window has no view of has sent it nothing.
    if (id in bytes) {
      total += bytes[id]
    }
  }
  return total
}

async function interruptAndAwaitQuiet(app: App, ids: string[]): Promise<number[]> {
  const call = `(() => {
    const ids = ${idList(ids)}
    for (const id of ids) {
      window.tacoShells.pty.write(id, ${JSON.stringify(ETX)})
    }
    return Promise.all(ids.map((id) => window.__measure.quietAfter(id, ${QUIET_MS})))
  })()`
  return inPage<number[]>(app, call)
}

async function interrupt(app: App, id: string): Promise<void> {
  const call = `window.tacoShells.pty.write(${JSON.stringify(id)}, ${JSON.stringify(ETX)})`
  await evaluate(app.page, call)
}

// A terminal the window has no view of sends it no output, so each shell says it
// has finished by making a file.
async function fillScrollback(app: App, ids: string[], markers: string): Promise<void> {
  const lineCount = SCROLLBACK + 10000
  const marked: string[] = []
  for (const id of ids) {
    const marker = join(markers, `filled-${id}`)
    marked.push(marker)
    await app.runInPane(id, `seq 1 ${lineCount} && touch '${marker}'`)
  }
  await pollFor(
    'every scrollback to be filled',
    async () => {
      const filled = marked.every((marker) => existsSync(marker))
      if (filled) {
        return true
      }
      return null
    },
    FILL_TIMEOUT_MS
  )
}

async function switchOnce(app: App, bracket: Key): Promise<number> {
  await evaluate(app.page, 'window.__measure.armSwitch()')
  await app.press(bracket, { cmd: true, shift: true })
  const elapsed = await inPage<number>(app, 'window.__measure.switched')
  await sleep(SWITCH_SETTLE_MS)
  return elapsed
}

async function openTerminals(app: App, terminals: number): Promise<void> {
  let rows = 1
  for (let opened = 0; opened < terminals - 2; opened += 1) {
    await app.press(KEY.t, { cmd: true })
    rows += 1
    const wanted = rows
    await app.until(`there are ${wanted} terminals`, (screen) => screen.rows.length === wanted)
  }
  await app.press(KEY.backslash, { cmd: true })
  await app.until(`there are ${terminals} terminals`, (screen) => screen.rows.length === terminals)
}

async function measure(
  app: App,
  flags: Flags,
  parameters: Parameters,
  markers: string
): Promise<Result> {
  const { terminals, full, flooding, floodSeconds, samples } = flags
  const typedId = terminalId(terminals)
  const floodIds = [terminalId(terminals - 1)]
  for (let number = 1; number < flooding; number += 1) {
    floodIds.push(terminalId(number))
  }
  const memory: MemoryStage[] = []

  await stage('install', async () => {
    await evaluate(app.page, PAGE_PROBE)
  })

  await stage('frame check', async () => {
    await evaluate(app.page, 'window.__measure.startLoad()')
    await sleep(FRAME_CHECK_MS)
    const sample = await inPage<FrameSample>(app, 'window.__measure.stopLoad()')
    const gaps = summarize(sample.frameGaps)
    if (sample.hidden || gaps.p50 > FRAME_CHECK_MAX_GAP_MS || gaps.count < FRAME_CHECK_MIN_GAPS) {
      throw new Error('the window is not drawing frames, so nothing measured here would mean anything')
    }
  })

  await stage('one terminal', async () => {
    const snapshot = await memorySnapshot(app, 'one terminal')
    memory.push(snapshot)
  })

  await stage('open', async () => {
    await openTerminals(app, terminals)
    const snapshot = await memorySnapshot(app, `${terminals} terminals, each shown once`)
    memory.push(snapshot)
  })

  const quiet = await stage('quiet', async () => {
    await app.until('the newest terminal has the keyboard', (screen) => screen.terminalFocused)
    await app.runInPane(typedId, 'cat')
    await sleep(QUIET_MS)
    await beginLoad(app)
    const typing = await typeTimes(app, typedId, samples)
    return endLoad(app, typing)
  })

  const flood = await stage('flood', async () => {
    const argument = 'y'.repeat(FLOOD_ARGUMENT_LENGTH)
    for (const id of floodIds) {
      await app.runInPane(id, `yes ${argument}`)
    }
    await sleep(FLOOD_WARMUP_MS)

    await beginLoad(app)
    const startedAt = Date.now()
    const charactersBefore = await receivedCharacters(app, floodIds)
    const typing = await typeUntil(app, typedId, startedAt + floodSeconds * 1000)
    const load = await endLoad(app, typing)
    const endedAt = Date.now()
    const charactersAfter = await receivedCharacters(app, floodIds)

    const seconds = (endedAt - startedAt) / 1000
    const megabytesPerSecond = (charactersAfter - charactersBefore) / seconds / BYTES_PER_MEGABYTE
    const quietTimes = await interruptAndAwaitQuiet(app, floodIds)
    const backlogMs = Math.max(...quietTimes)
    await interrupt(app, typedId)
    const result: FloodLoad = { ...load, megabytesPerSecond, backlogMs }
    return result
  })

  await stage('fill', async () => {
    const fillIds: string[] = []
    for (let number = flooding; number < full; number += 1) {
      fillIds.push(terminalId(number))
    }
    await fillScrollback(app, fillIds, markers)

    for (let attempt = 0; attempt < QUIET_WINDOW_TRIES; attempt += 1) {
      await evaluate(app.page, 'window.__measure.startLoad()')
      await sleep(QUIET_WINDOW_MS)
      const sample = await inPage<FrameSample>(app, 'window.__measure.stopLoad()')
      if (sample.longTasks === 0) {
        break
      }
    }
    const snapshot = await memorySnapshot(app, `${full} terminals with a full scrollback`)
    memory.push(snapshot)
  })

  const everyEntryInTurnMs = await stage('switch, every entry in turn', async () => {
    const elapsed: number[] = []
    for (let step = 0; step < terminals - 2; step += 1) {
      const took = await switchOnce(app, KEY.bracketLeft)
      elapsed.push(took)
    }
    for (let step = 0; step < terminals - 2; step += 1) {
      const took = await switchOnce(app, KEY.bracketRight)
      elapsed.push(took)
    }
    return elapsed
  })

  const betweenTwoMs = await stage('switch, between two entries', async () => {
    const elapsed: number[] = []
    for (let pair = 0; pair < SWITCH_PAIRS; pair += 1) {
      const back = await switchOnce(app, KEY.bracketLeft)
      const forward = await switchOnce(app, KEY.bracketRight)
      elapsed.push(back, forward)
    }
    return elapsed
  })

  const graphics = await stage('graphics', async () => {
    const contextsLost = await inPage<number>(app, 'window.__measure.contextsLost')
    const onSlowRenderer = await inPage<number>(app, 'window.__measure.onSlowRenderer()')
    const snapshot = await memorySnapshot(app, 'at the end')
    memory.push(snapshot)
    return { contextsLost, onSlowRenderer }
  })

  const switching = {
    everyEntryInTurnMs: summarize(everyEntryInTurnMs),
    betweenTwoMs: summarize(betweenTwoMs)
  }
  const now = new Date()
  const date = now.toISOString().slice(0, 10)
  const commit = shortCommit()
  const machine = describeMachine()
  return {
    date,
    commit,
    machine,
    parameters,
    memory,
    quiet,
    flood,
    switching,
    graphics
  }
}

function shortCommit(): string {
  const output = execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: REPO_ROOT, encoding: 'utf8' })
  return output.trim()
}

function describeMachine(): string {
  const processors = cpus()
  const model = processors[0].model
  const gigabytes = Math.round(totalmem() / (1024 * 1024 * 1024))
  const versionOutput = execFileSync('sw_vers', ['-productVersion'], { encoding: 'utf8' })
  const version = versionOutput.trim()
  return `${model}, ${processors.length} cores, ${gigabytes} GB, macOS ${version}`
}

async function main(): Promise<void> {
  const flags = parseFlags(process.argv.slice(2))
  await refuseIfPortTaken()

  const parameters: Parameters = {
    terminals: flags.terminals,
    full: flags.full,
    flooding: flags.flooding,
    floodSeconds: flags.floodSeconds,
    samples: flags.samples,
    scrollback: SCROLLBACK,
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT
  }

  const state = mkdtempSync(join(tmpdir(), 'taco-shells-measure-'))
  const configDir = join(state, 'config')
  const zdotdir = join(state, 'zdotdir')
  mkdirSync(configDir)
  mkdirSync(zdotdir)
  writeFileSync(join(zdotdir, '.zshrc'), '')

  let app: App | null = null
  try {
    app = await launchApp({
      configDir,
      window: { width: WINDOW_WIDTH, height: WINDOW_HEIGHT, parked: true },
      env: { ZDOTDIR: zdotdir }
    })
    const result = await measure(app, flags, parameters, state)

    mkdirSync(RESULT_DIR, { recursive: true })
    const path = join(RESULT_DIR, `${result.date}-${result.commit}.json`)
    writeFileSync(path, JSON.stringify(result, null, 2))
    console.log(formatReport(result))
    console.error(`wrote ${path}`)
  } finally {
    if (app !== null) {
      await app.close()
    }
    rmSync(state, { recursive: true, force: true })
  }
}

try {
  await main()
} catch (failure) {
  let message = String(failure)
  if (failure instanceof Error) {
    message = failure.message
  }
  console.error(message)
  process.exitCode = 1
}
