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
  }
}

// True tells xterm the sequence is dealt with, so its own handler, the one that answers, does not run.
function answeredElsewhere(): boolean {
  return true
}

/** Stops a view answering what the runtime's screen answers. What only a window knows (focus, colors) it still answers. */
export function leaveAnswersToTheRuntime(term: AnsweringTerminal): void {
  for (const question of QUESTIONS) {
    term.parser.registerCsiHandler(question, answeredElsewhere)
  }
  term.parser.registerDcsHandler(STATUS_STRING_REQUEST, answeredElsewhere)
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
