import type { TerminalColors } from '@protocol/theme'

type ColorSequence = 4 | 10 | 11 | 12
type Rgb = [number, number, number]

const QUERY = '?'
const FIRST_DYNAMIC = 10
const LAST_DYNAMIC = 12
const DYNAMIC_COLORS: (keyof TerminalColors)[] = ['foreground', 'background', 'cursor']
const THEME_SIXTEEN: (keyof TerminalColors)[] = [
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
  'brightBlack',
  'brightRed',
  'brightGreen',
  'brightYellow',
  'brightBlue',
  'brightMagenta',
  'brightCyan',
  'brightWhite'
]
const FIRST_CUBE = 16
const FIRST_GRAY = 232
const LAST_INDEX = 255
const CUBE_LEVELS = [0, 95, 135, 175, 215, 255]
const CUBE_SIDE = 6
const HEX_COLOR = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i
const WHOLE_NUMBER = /^\d+$/

function parseHex(color: string): Rgb | null {
  const match = HEX_COLOR.exec(color)
  if (match === null) {
    return null
  }
  const red = parseInt(match[1], 16)
  const green = parseInt(match[2], 16)
  const blue = parseInt(match[3], 16)
  return [red, green, blue]
}

function paletteColor(index: number, colors: TerminalColors): Rgb | null {
  if (index < FIRST_CUBE) {
    const name = THEME_SIXTEEN[index]
    return parseHex(colors[name])
  }
  if (index < FIRST_GRAY) {
    const offset = index - FIRST_CUBE
    const red = Math.floor(offset / (CUBE_SIDE * CUBE_SIDE))
    const green = Math.floor(offset / CUBE_SIDE) % CUBE_SIDE
    const blue = offset % CUBE_SIDE
    return [CUBE_LEVELS[red], CUBE_LEVELS[green], CUBE_LEVELS[blue]]
  }
  const gray = 8 + 10 * (index - FIRST_GRAY)
  return [gray, gray, gray]
}

function channel(value: number): string {
  const digits = value.toString(16).padStart(2, '0')
  return digits + digits
}

function answer(ident: string, rgb: Rgb): string {
  const [red, green, blue] = rgb
  return `\x1b]${ident};rgb:${channel(red)}/${channel(green)}/${channel(blue)}\x1b\\`
}

function dynamicAnswers(first: number, data: string, colors: TerminalColors): string[] {
  const answers: string[] = []
  const slots = data.split(';')
  for (let position = 0; position < slots.length; position += 1) {
    const ident = first + position
    if (ident > LAST_DYNAMIC) {
      break
    }
    if (slots[position] !== QUERY) {
      continue
    }
    const name = DYNAMIC_COLORS[ident - FIRST_DYNAMIC]
    const rgb = parseHex(colors[name])
    if (rgb !== null) {
      answers.push(answer(String(ident), rgb))
    }
  }
  return answers
}

function paletteAnswers(data: string, colors: TerminalColors): string[] {
  const answers: string[] = []
  const parts = data.split(';')
  for (let position = 0; position + 1 < parts.length; position += 2) {
    const spec = parts[position + 1]
    if (spec !== QUERY || !WHOLE_NUMBER.test(parts[position])) {
      continue
    }
    const index = parseInt(parts[position], 10)
    if (index > LAST_INDEX) {
      continue
    }
    const rgb = paletteColor(index, colors)
    if (rgb !== null) {
      answers.push(answer(`4;${index}`, rgb))
    }
  }
  return answers
}

export function colorAnswers(sequence: ColorSequence, data: string, colors: TerminalColors): string[] {
  if (sequence === 4) {
    return paletteAnswers(data, colors)
  }
  return dynamicAnswers(sequence, data, colors)
}
