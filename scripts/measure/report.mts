import type { Summary } from './stats.mts'

export interface ProcessMemory {
  kind: string
  megabytes: number
}

export interface MemoryStage {
  stage: string
  processes: ProcessMemory[]
  totalMegabytes: number
  windowHeapMegabytes: number
}

export interface Load {
  typingMs: Summary
  mainLagMs: Summary
  frameGapMs: Summary
  longTasks: number
}

export interface FloodLoad extends Load {
  megabytesPerSecond: number
  backlogMs: number
}

export interface Parameters {
  terminals: number
  full: number
  flooding: number
  floodSeconds: number
  samples: number
  scrollback: number
  width: number
  height: number
}

export interface Result {
  date: string
  commit: string
  machine: string
  parameters: Parameters
  memory: MemoryStage[]
  quiet: Load
  flood: FloodLoad
  switching: { everyEntryInTurnMs: Summary; betweenTwoMs: Summary }
  graphics: { contextsLost: number; onSlowRenderer: number }
}

type Row = [label: string, value: string]

function milliseconds(value: number): string {
  return value.toFixed(1)
}

function megabytes(value: number): string {
  return value.toFixed(0)
}

function spread(summary: Summary): string {
  const median = milliseconds(summary.p50)
  const slowest = milliseconds(summary.p95)
  const worst = milliseconds(summary.max)
  return `${median} / ${slowest} / ${worst}`
}

function memoryRows(stage: MemoryStage): Row[] {
  const rows: Row[] = []
  for (const entry of stage.processes) {
    rows.push([`Memory, ${stage.stage}: ${entry.kind}`, megabytes(entry.megabytes)])
  }
  rows.push([`Memory, ${stage.stage}: total`, megabytes(stage.totalMegabytes)])
  rows.push([`Memory, ${stage.stage}: window's JavaScript heap`, megabytes(stage.windowHeapMegabytes)])
  return rows
}

function loadRows(load: Load, when: string): Row[] {
  return [
    [`Typing delay, ${when}: median / 95th / worst (ms)`, spread(load.typingMs)],
    [`Main process timer lag, ${when}: median / 95th / worst (ms)`, spread(load.mainLagMs)],
    [`Frame gap, ${when}: median / 95th / worst (ms)`, spread(load.frameGapMs)],
    [`Long tasks over 50 ms, ${when}`, String(load.longTasks)]
  ]
}

function interleaved(quiet: Row[], flood: Row[]): Row[] {
  const rows: Row[] = []
  for (let index = 0; index < quiet.length; index += 1) {
    rows.push(quiet[index])
    rows.push(flood[index])
  }
  return rows
}

export function formatReport(result: Result): string {
  const rows: Row[] = []
  for (const stage of result.memory) {
    rows.push(...memoryRows(stage))
  }

  const quietRows = loadRows(result.quiet, 'nothing printing')
  const floodRows = loadRows(result.flood, 'while printing')
  rows.push(...interleaved(quietRows, floodRows))

  const rate = result.flood.megabytesPerSecond.toFixed(1)
  rows.push(['Output reaching the window while printing (MB/s)', rate])
  rows.push(['Backlog after printing stopped (ms)', milliseconds(result.flood.backlogMs)])
  rows.push(['Switch, every entry in turn: median / 95th / worst (ms)', spread(result.switching.everyEntryInTurnMs)])
  rows.push(['Switch, between two entries: median / 95th / worst (ms)', spread(result.switching.betweenTwoMs)])
  rows.push(['Graphics contexts lost', String(result.graphics.contextsLost)])
  rows.push(['Terminals on the slow renderer', String(result.graphics.onSlowRenderer)])

  const parameters = result.parameters
  const settings = `${parameters.terminals} terminals, ${parameters.full} with a full scrollback, ${parameters.flooding} printing for ${parameters.floodSeconds} s, ${parameters.samples} typing samples, scrollback ${parameters.scrollback}, window ${parameters.width}x${parameters.height}`
  const lines = [`${settings}. Machine: ${result.machine}.`, '']
  lines.push(`| Measure | ${result.date} ${result.commit} |`)
  lines.push('| --- | --- |')
  for (const [label, value] of rows) {
    lines.push(`| ${label} | ${value} |`)
  }
  return lines.join('\n')
}
