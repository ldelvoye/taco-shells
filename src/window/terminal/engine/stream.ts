import type { PtySize } from '@protocol/ipc'

export interface StreamTerminal {
  write(data: string, callback?: () => void): void
  resize(cols: number, rows: number): void
  clear(): void
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
