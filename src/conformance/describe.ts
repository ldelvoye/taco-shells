import type { IBuffer, IBufferLine, Terminal } from '@xterm/headless'

export interface Description {
  active: string
  cursorX: number
  cursorY: number
  cursorHidden: boolean
  normal: string[]
  alternate: string[]
  modes: Terminal['modes']
  cells: string[][]
}

function textOf(buffer: IBuffer): string[] {
  const lines: string[] = []
  for (let y = 0; y < buffer.length; y += 1) {
    const line = buffer.getLine(y)
    if (line === undefined) {
      continue
    }
    lines.push(line.translateToString(true))
  }
  return lines
}

function cellsOf(line: IBufferLine, cols: number): string[] {
  const cells: string[] = []
  for (let x = 0; x < cols; x += 1) {
    const cell = line.getCell(x)
    if (cell === undefined) {
      continue
    }
    const foreground = `${cell.getFgColorMode()}:${cell.getFgColor()}`
    const background = `${cell.getBgColorMode()}:${cell.getBgColor()}`
    cells.push(`${foreground}/${background}/${cell.isBold()}`)
  }
  return cells
}

function screenCells(term: Terminal): string[][] {
  const buffer = term.buffer.active
  const rows: string[][] = []
  for (let y = 0; y < term.rows; y += 1) {
    const line = buffer.getLine(buffer.baseY + y)
    if (line === undefined) {
      continue
    }
    rows.push(cellsOf(line, term.cols))
  }
  return rows
}

export function describeTerminal(term: Terminal, cursorHidden: boolean): Description {
  const active = term.buffer.active
  return {
    active: active.type,
    cursorX: active.cursorX,
    cursorY: active.cursorY,
    cursorHidden,
    normal: textOf(term.buffer.normal),
    alternate: textOf(term.buffer.alternate),
    modes: term.modes,
    cells: screenCells(term)
  }
}
