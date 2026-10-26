// A taco in the terminal with a ring of text orbiting it: the text passes in
// front of the taco bright and behind it dim, hidden where the taco covers it.
// One full orbit takes PERIOD_MS in exactly PERIOD_MS * FPS / 1000 steps, and
// each step is drawn once, so a recording can cut one orbit frame for frame.

const PERIOD_MS = 12000
const FPS = 30
const STEPS = (PERIOD_MS * FPS) / 1000
const PHRASE = 'TACO SHELLS  ·  '
const RING_MARGIN = 12
const RING_RISE = 6
const LETTER_SPACING = 1.25
const TITLE = '🌮 taco'

const TACO_COLOUR = [226, 168, 72]
const TACO_SHADE_TOP = 1
const TACO_SHADE_BOTTOM = 0.6
const TEXT_SHADE_FRONT = 1
const TEXT_SHADE_BACK = 0.3
const RAINBOW_CYCLES = 2
const RAINBOW_SATURATION = 0.85

const BRAILLE_BLANK = '⠀'
const TACO = [
  '⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⠀⠀⢀⣤⣀⠀⠀⢀⣴⢶⣄⠀⠀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀',
  '⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣀⠀⠀⣠⡾⠛⢷⣄⣿⠁⠉⠻⠾⠛⠁⠀⠙⢛⣛⠻⣧⣤⣄⠀⠀⠀⠀⠀⠀⠀⠀',
  '⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⠉⠉⠛⠛⠋⠀⢀⣀⣉⢀⣠⣤⣴⠛⠲⣤⡖⠋⠉⠈⠙⣦⣦⣿⣇⠀⠀⠀⠀⠀⠀⠀',
  '⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⡴⠟⠀⠀⣀⣀⣀⣴⣋⡀⠉⠉⠀⠀⣹⡶⢟⠛⠛⡛⢶⣄⣠⣿⣇⡀⠈⠙⡷⣆⠀⠀⠀⠀',
  '⠀⠀⠀⠀⠀⠀⠀⠀⠀⣴⠟⢻⣤⣤⡶⠟⠛⠛⡙⠛⢻⣿⣿⣶⣤⣾⣯⣤⠾⠷⠶⠷⠶⠿⠿⣯⣬⣻⣦⣼⡷⠟⢻⡆⠀⠀',
  '⠀⠀⠀⠀⠀⠀⢀⣴⠾⠃⢀⣴⠟⠉⠀⠰⠄⢀⡧⠤⣞⣠⡾⠟⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠉⠛⢷⣄⡀⠘⣷⡀⠀',
  '⠀⠀⠀⠀⠀⠀⣿⠁⠀⢀⣿⠏⠀⠀⠃⣠⣴⣾⣦⡾⠋⠁⠀⠀⠀⠀⠀⠀⣠⡤⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠻⣦⠉⢿⡆',
  '⠀⠀⠀⠀⣠⡾⠋⠀⠀⣸⣿⡀⠘⠃⠸⢿⣿⠟⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠹⠷⠀⠀⠀⠀⠀⣾⡃⠀⠀⠀⠀⠀⠙⣧⠙⣷',
  '⠀⠀⠀⠀⣿⠀⠀⠀⠀⣻⣿⣿⣿⣷⣿⡿⠋⠀⠀⠀⠀⠀⢀⣶⠂⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠋⠀⠀⠀⠀⠀⠀⠸⣿⠋',
  '⠀⠀⠀⠀⣼⠷⢶⣴⡾⠿⣿⡏⠀⣴⠏⠀⠀⠀⣤⠀⠀⠀⠈⠻⠆⠀⠀⠀⠀⠀⢸⣇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢿⠀',
  '⠀⠀⠀⣸⠏⣰⡿⠋⢦⣠⡼⢇⣼⠃⠀⠀⠀⠀⠛⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢠⣿⠂⠀⠀⠀⠀⣼⠀',
  '⠀⠀⣰⠏⣰⡟⠀⠠⢀⣯⣀⣾⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣤⡄⠀⠀⠀⠀⠀⠀⠀⠀⠙⠁⠀⠀⠀⢠⡟⠀',
  '⠀⢠⣟⣠⣿⠀⠰⠀⣿⣿⣿⠃⠀⠀⠀⠀⠀⣼⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠻⠆⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣤⡴⠾⠋⠀⠀',
  '⠀⣾⣹⠉⣿⣦⣤⠴⠒⣿⠃⠀⠀⠀⠀⠀⠀⠻⠖⠀⠀⠀⠀⣷⡀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣤⡴⠶⠛⠋⠉⠀⠀⠀⠀⠀⠀',
  '⢰⡟⣏⠀⢻⣟⠁⣀⣼⠇⠀⠀⢀⡄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣀⣤⡴⠶⠛⠛⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀',
  '⣾⢷⡋⠇⠈⢿⣦⣰⠏⠀⠀⠀⠘⠧⠀⠀⠀⠀⠀⢀⣀⣠⣤⠶⠞⠛⠉⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀',
  '⣿⢸⣇⠸⢢⠀⣹⡟⠀⠀⠀⠀⠀⢀⣀⣤⡴⠶⠛⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀',
  '⢻⣟⢘⡆⢈⣴⠏⢀⣀⣤⡴⠶⠛⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀',
  '⠀⠙⠷⠶⠿⠷⠛⠋⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀'
]
const TACO_WIDTH = Math.max(...TACO.map((line) => [...line].length))
const TACO_HEIGHT = TACO.length
const RING_RADIUS = TACO_WIDTH / 2 + RING_MARGIN
const RING_LETTERS = ringLetters()

function ringLetters() {
  const circumference = 2 * Math.PI * RING_RADIUS
  const repeats = Math.max(1, Math.round(circumference / LETTER_SPACING / PHRASE.length))
  return [...PHRASE.repeat(repeats)]
}

function gridSize() {
  let columns = 100
  let rows = 36
  if (process.stdout.columns) {
    columns = process.stdout.columns
  }
  if (process.stdout.rows) {
    rows = process.stdout.rows
  }
  return { columns, rows }
}

function colourCode(colour, shade) {
  const red = Math.round(colour[0] * shade)
  const green = Math.round(colour[1] * shade)
  const blue = Math.round(colour[2] * shade)
  return `\x1b[38;2;${red};${green};${blue}m`
}

// A hue from 0 to 1 at full value, so the ring runs through the spectrum.
function rainbow(hue) {
  const sector = (hue % 1) * 6
  const wedge = Math.floor(sector)
  const fraction = sector - wedge
  const low = Math.round(255 * (1 - RAINBOW_SATURATION))
  const rising = Math.round(255 * (1 - RAINBOW_SATURATION * (1 - fraction)))
  const falling = Math.round(255 * (1 - RAINBOW_SATURATION * fraction))
  const wedges = [
    [255, rising, low],
    [falling, 255, low],
    [low, 255, rising],
    [low, falling, 255],
    [rising, low, 255],
    [255, low, falling]
  ]
  return wedges[wedge]
}

function blankCells(columns, rows) {
  return new Array(columns * rows).fill(null)
}

function placeTaco(cells, columns, rows) {
  const left = Math.round((columns - TACO_WIDTH) / 2)
  const top = Math.round((rows - TACO_HEIGHT) / 2)
  for (let lineIndex = 0; lineIndex < TACO_HEIGHT; lineIndex += 1) {
    const glyphs = [...TACO[lineIndex]]
    const shade = TACO_SHADE_TOP - ((TACO_SHADE_TOP - TACO_SHADE_BOTTOM) * lineIndex) / (TACO_HEIGHT - 1)
    const row = top + lineIndex
    for (let glyphIndex = 0; glyphIndex < glyphs.length; glyphIndex += 1) {
      const glyph = glyphs[glyphIndex]
      if (glyph === BRAILLE_BLANK) {
        continue
      }
      const column = left + glyphIndex
      const inside = row >= 0 && row < rows && column >= 0 && column < columns
      if (inside) {
        cells[row * columns + column] = { glyph, colour: TACO_COLOUR, shade }
      }
    }
  }
}

// Letters sit on a circle around the taco, seen from slightly above: the
// front of the ring is lower on screen than the back, and runs left to
// right so the phrase reads as it passes. Each letter is drawn only over an
// empty cell when behind the taco, and over anything when in front.
function placeRing(cells, columns, rows, turn) {
  const centreColumn = columns / 2
  const centreRow = rows / 2
  const letters = RING_LETTERS.map((glyph, index) => {
    const angle = turn * 2 * Math.PI + (index / RING_LETTERS.length) * 2 * Math.PI
    const depth = Math.sin(angle)
    const column = Math.round(centreColumn - RING_RADIUS * Math.cos(angle))
    const row = Math.round(centreRow + RING_RISE * depth)
    const colour = rainbow((index / RING_LETTERS.length) * RAINBOW_CYCLES)
    return { glyph, column, row, depth, colour }
  })
  letters.sort((a, b) => a.depth - b.depth)

  for (const letter of letters) {
    if (letter.glyph === ' ') {
      continue
    }
    const outside = letter.row < 0 || letter.row >= rows || letter.column < 0 || letter.column >= columns
    if (outside) {
      continue
    }
    const cell = letter.row * columns + letter.column
    const behind = letter.depth < 0
    const coveredByTaco = cells[cell] !== null && cells[cell].colour === TACO_COLOUR
    if (behind && coveredByTaco) {
      continue
    }
    const shade = TEXT_SHADE_BACK + ((TEXT_SHADE_FRONT - TEXT_SHADE_BACK) * (letter.depth + 1)) / 2
    cells[cell] = { glyph: letter.glyph, colour: letter.colour, shade }
  }
}

function renderFrame(turn) {
  const { columns, rows } = gridSize()
  const cells = blankCells(columns, rows)
  placeTaco(cells, columns, rows)
  placeRing(cells, columns, rows, turn)

  const lines = []
  let current = ''
  for (let row = 0; row < rows; row += 1) {
    let line = ''
    for (let column = 0; column < columns; column += 1) {
      const cell = cells[row * columns + column]
      if (cell === null) {
        line += ' '
        continue
      }
      const code = colourCode(cell.colour, cell.shade)
      if (code !== current) {
        line += code
        current = code
      }
      line += cell.glyph
    }
    lines.push(line)
  }
  return `\x1b[H${lines.join('\n')}\x1b[0m`
}

function showCursorAndExit() {
  process.stdout.write('\x1b[?25h\x1b[0m\n')
  process.exit(0)
}

process.on('SIGINT', showCursorAndExit)
process.on('SIGTERM', showCursorAndExit)

process.stdout.write(`\x1b]0;${TITLE}\x07\x1b[?25l\x1b[2J`)
const started = Date.now()
let drawnStep = -1
setInterval(() => {
  const elapsed = Date.now() - started
  const step = Math.floor((elapsed / PERIOD_MS) * STEPS) % STEPS
  if (step === drawnStep) {
    return
  }
  drawnStep = step
  process.stdout.write(renderFrame(step / STEPS))
}, 1000 / FPS / 2)
