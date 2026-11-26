import type { PtySize } from '@protocol/ipc'

export interface Screen {
  /** Feeds output. `parsed` runs once this piece has been taken in. */
  write(data: string, parsed: () => void): void
  /** Runs `then` once everything written so far has been taken in, and before anything written later. */
  afterParsed(then: () => void): void
  /** Acts at once: call it from `afterParsed` to place it in the output. */
  resize(size: PtySize): void
  clear(): void
  setScrollback(lines: number): void
  /** What a fresh view needs to show what this screen shows. Opaque outside the engine pair. */
  snapshot(): string
  onTitle(listener: (title: string) => void): void
  /** What the terminal answers when a program asks it something. It goes to the shell as typed input does. */
  onReply(listener: (data: string) => void): void
  dispose(): void
}

export interface ScreenOptions {
  size: PtySize
  scrollback: number
}

export type MakeScreen = (options: ScreenOptions) => Screen
