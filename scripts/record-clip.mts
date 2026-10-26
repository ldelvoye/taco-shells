import { execFile } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { promisify } from 'node:util'

import { type App, KEY, launchApp } from '../e2e/app.ts'

const runCommand = promisify(execFile)

const REPO_ROOT = resolve(import.meta.dirname, '..')
const CLIP_DIR = join(REPO_ROOT, 'scripts/clip')
const OUTPUT_DIR = join(REPO_ROOT, 'dist.noindex')

const WIDTH = 1280
const HEIGHT = 720
const JPEG_QUALITY = 95
const OUT_WIDTH = 1920
const OUT_HEIGHT = 1080
const OUT_FPS = 30
const LAST_FRAME_MS = 400

// The walkthrough, as holds between keystrokes: the Claude pane alone, a second
// terminal, that terminal split, then back to the Claude pane as its run ends.
// The transcripts in scripts/clip/walkthrough are timed against these.
const OPENING_MS = 3200
const SECOND_TERMINAL_MS = 3200
const SPLIT_MS = 3600
const RETURN_MS = 3600

// The taco's ring orbits once per period, drawing one frame per output frame;
// the clip keeps exactly one orbit's worth of captured frames so it loops
// without a seam, starting a little in so the pane has settled.
const TACO_PERIOD_MS = 12000
const TACO_LEAD_MS = 1500
const TACO_TAIL_MS = 1000

interface Frame {
  path: string
  at: number
}

interface ScreencastFrame {
  data: string
  sessionId: number
  metadata: { timestamp?: number }
}

interface Scene {
  settings: Record<string, unknown> | null
  loopPeriodMs: number | null
  take(app: App): Promise<void>
}

const SCENES: Record<string, Scene> = {
  walkthrough: {
    settings: null,
    loopPeriodMs: null,
    async take(app) {
      await app.pause(OPENING_MS)
      await app.press(KEY.t, { cmd: true })
      await app.until('the second terminal is open', (screen) => screen.rows.length === 2)
      await app.pause(SECOND_TERMINAL_MS)
      await app.press(KEY.backslash, { cmd: true })
      await app.until('the second terminal is split', (screen) => screen.rows.length === 3)
      await app.pause(SPLIT_MS)
      await app.press(KEY.bracketLeft, { cmd: true, shift: true })
      await app.until('the Claude pane has the keyboard', (screen) => screen.rows[0].active)
      await app.pause(RETURN_MS)
    }
  },
  taco: {
    settings: { theme: 'dark' },
    loopPeriodMs: TACO_PERIOD_MS,
    async take(app) {
      await app.pause(TACO_LEAD_MS + TACO_PERIOD_MS + TACO_TAIL_MS)
    }
  }
}

interface Timed {
  path: string
  seconds: number
}

// Frames as captured, each held until the next one arrived.
function timedByCapture(frames: Frame[]): Timed[] {
  const timed: Timed[] = []
  for (let index = 0; index < frames.length; index += 1) {
    const frame = frames[index]
    const next = frames[index + 1]
    let seconds = LAST_FRAME_MS / 1000
    if (next !== undefined) {
      seconds = next.at - frame.at
    }
    timed.push({ path: frame.path, seconds })
  }
  return timed
}

// One orbit, frame for frame: the scene draws one state per output frame, so
// the captured frames after the lead are taken as consecutive output frames.
// The frame right after the cut must repeat the first one for the loop to be
// seamless, and that is checked byte for byte.
function oneLoop(frames: Frame[], periodMs: number): { timed: Timed[]; seamless: boolean | null } {
  const start = frames[0].at + TACO_LEAD_MS / 1000
  const first = frames.findIndex((frame) => frame.at >= start)
  const count = Math.round((periodMs / 1000) * OUT_FPS)
  const kept = frames.slice(first, first + count)
  if (kept.length < count) {
    throw new Error(`the capture holds ${kept.length} frames after the lead, one loop needs ${count}`)
  }
  const timed = kept.map((frame) => ({ path: frame.path, seconds: 1 / OUT_FPS }))

  const wrap = frames[first + count]
  let seamless: boolean | null = null
  if (wrap !== undefined) {
    seamless = readFileSync(wrap.path).equals(readFileSync(kept[0].path))
  }
  return { timed, seamless }
}

function listing(timed: Timed[]): string {
  const lines: string[] = []
  for (const entry of timed) {
    lines.push(`file '${entry.path}'`)
    lines.push(`duration ${entry.seconds.toFixed(4)}`)
  }
  const final = timed[timed.length - 1]
  lines.push(`file '${final.path}'`)
  return lines.join('\n')
}

async function capture(sceneName: string, scene: Scene, directory: string): Promise<Frame[]> {
  const state = await mkdtemp(join(tmpdir(), 'taco-shells-clip-state-'))
  const configDir = join(state, 'config')
  await mkdir(configDir)
  if (scene.settings !== null) {
    await writeFile(join(configDir, 'settings.json'), JSON.stringify(scene.settings))
  }
  const app = await launchApp({
    configDir,
    window: { width: WIDTH, height: HEIGHT },
    env: { ZDOTDIR: CLIP_DIR, TACO_CLIP_SCENE: join(CLIP_DIR, sceneName), TACO_CLIP_STATE: state }
  })
  const frames: Frame[] = []

  app.onCdpEvent('Page.screencastFrame', (params) => {
    const frame = params as unknown as ScreencastFrame
    const acknowledge = () => {
      app.cdp('Page.screencastFrameAck', { sessionId: frame.sessionId }).catch(() => {})
    }
    const at = frame.metadata.timestamp
    if (at === undefined) {
      acknowledge()
      return
    }
    const path = join(directory, `${String(frames.length).padStart(5, '0')}.jpg`)
    writeFileSync(path, Buffer.from(frame.data, 'base64'))
    frames.push({ path, at })
    acknowledge()
  })

  await app.cdp('Page.startScreencast', {
    format: 'jpeg',
    quality: JPEG_QUALITY,
    maxWidth: WIDTH * 2,
    maxHeight: HEIGHT * 2,
    everyNthFrame: 1
  })

  await scene.take(app)

  await app.cdp('Page.stopScreencast')
  await app.close()
  await rm(state, { recursive: true, force: true })

  if (frames.length === 0) {
    throw new Error('the screencast produced no frames')
  }
  return frames
}

async function encode(list: string, frameCount: number, target: string): Promise<void> {
  const filter = `scale=${OUT_WIDTH}:${OUT_HEIGHT}:flags=lanczos,fps=${OUT_FPS}`
  const args = [
    '-y',
    '-f',
    'concat',
    '-safe',
    '0',
    '-i',
    list,
    '-frames:v',
    String(frameCount),
    '-vf',
    filter,
    '-an',
    '-c:v',
    'libx264',
    '-preset',
    'slow',
    '-crf',
    '21',
    '-profile:v',
    'high',
    '-pix_fmt',
    'yuv420p',
    '-movflags',
    '+faststart',
    target
  ]
  await runCommand('ffmpeg', args, { maxBuffer: 1024 * 1024 * 64 })
}

let sceneName = 'walkthrough'
if (process.argv[2] !== undefined) {
  sceneName = process.argv[2]
}
const scene = SCENES[sceneName]
if (scene === undefined) {
  throw new Error(`no scene named ${sceneName}; pick one of ${Object.keys(SCENES).join(', ')}`)
}
const target = join(OUTPUT_DIR, `clip-${sceneName}.mp4`)

const directory = await mkdtemp(join(tmpdir(), 'taco-shells-clip-'))
try {
  const captured = await capture(sceneName, scene, directory)
  let timed = timedByCapture(captured)
  let seamless: boolean | null = null
  if (scene.loopPeriodMs !== null) {
    const loop = oneLoop(captured, scene.loopPeriodMs)
    timed = loop.timed
    seamless = loop.seamless
  }
  const totalSeconds = timed.reduce((sum, entry) => sum + entry.seconds, 0)
  const frameCount = Math.round(totalSeconds * OUT_FPS)
  const list = join(directory, 'frames.txt')
  await writeFile(list, listing(timed))
  await mkdir(dirname(target), { recursive: true })
  await encode(list, frameCount, target)
  console.log(`wrote ${target}: ${frameCount} frames from ${timed.length} captured`)
  if (seamless === true) {
    console.log('loop check: the frame after the cut repeats the first, the loop is seamless')
  }
  if (seamless === false) {
    console.log('loop check: the frame after the cut differs from the first, the loop will jump')
  }
  if (seamless === null && scene.loopPeriodMs !== null) {
    console.log('loop check: no frame captured after the cut, the loop is unverified')
  }
} finally {
  await rm(directory, { recursive: true, force: true })
}
