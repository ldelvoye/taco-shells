import type { PtySize } from '@protocol/ipc'

// Every question xterm answers from its parser: device attributes, device status
// and cursor position, and mode reports. Read from InputHandler.ts at xterm 6.0.0;
// one missing here is answered twice, by the runtime's screen and by the view.
const QUESTIONS: SequenceId[] = [
  { final: 'c' },
  { prefix: '>', final: 'c' },
  { final: 'n' },
  { prefix: '?', final: 'n' },
  { intermediates: '$', final: 'p' },
  { prefix: '?', intermediates: '$', final: 'p' }
]
const STATUS_STRING_REQUEST: SequenceId = { intermediates: '$', final: 'q' }
const COLOR_SEQUENCES: (4 | 10 | 11 | 12)[] = [4, 10, 11, 12]
const COLOR_QUERY = '?'

export interface StreamTerminal {
  write(data: string, callback?: () => void): void
  resize(cols: number, rows: number): void
  clear(): void
}

interface SequenceId {
  prefix?: string
  intermediates?: string
  final: string
}

export interface AnsweringTerminal {
  parser: {
    registerCsiHandler(id: SequenceId, callback: () => boolean): unknown
    registerDcsHandler(id: SequenceId, callback: () => boolean): unknown
    registerOscHandler(ident: number, callback: (data: string) => boolean): unknown
  }
}

// True tells xterm the sequence is dealt with, so its own handler, the one that answers, does not run.
function answeredElsewhere(): boolean {
  return true
}

// A color sequence asks when a color is `?`, and sets one otherwise. The runtime answers
// the asking, so a view takes only those away from xterm and still acts on a color being set.
export function holdsColorQuery(sequence: 4 | 10 | 11 | 12, data: string): boolean {
  const parts = data.split(';')
  if (sequence === 4) {
    // The palette sequence comes in `index;color` pairs.
    const specs = parts.filter((_part, position) => position % 2 === 1)
    return specs.includes(COLOR_QUERY)
  }
  return parts.includes(COLOR_QUERY)
}

/** Stops a view answering what the runtime's screen answers. What only a window knows (whether it has focus) it still reports. */
export function leaveAnswersToTheRuntime(term: AnsweringTerminal): void {
  for (const question of QUESTIONS) {
    term.parser.registerCsiHandler(question, answeredElsewhere)
  }
  term.parser.registerDcsHandler(STATUS_STRING_REQUEST, answeredElsewhere)
  for (const sequence of COLOR_SEQUENCES) {
    term.parser.registerOscHandler(sequence, (data) => holdsColorQuery(sequence, data))
  }
}

export function loadSnapshot(term: StreamTerminal, snapshot: string, size: PtySize): void {
  term.resize(size.cols, size.rows)
  term.write(snapshot)
}

export function resizeInOrder(term: StreamTerminal, size: PtySize): void {
  term.write('', () => {
    term.resize(size.cols, size.rows)
  })
}

export function clearInOrder(term: StreamTerminal): void {
  term.write('', () => {
    term.clear()
  })
}
