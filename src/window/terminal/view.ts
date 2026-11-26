import type { PtySize } from '@protocol/ipc'
import type { Settings } from '@protocol/settings'
import type { TerminalColors } from '@protocol/theme'

export interface TerminalView {
  readonly element: HTMLElement
  /** Puts the view in the page. Safe to call once the element is attached. */
  mount(): void
  load(snapshot: string, size: PtySize): void
  write(data: string): void
  /** Both take effect after the output written so far. */
  resize(size: PtySize): void
  clear(): void
  /** The grid that fits the element now, or null when it has no layout. */
  fittingSize(): PtySize | null
  focus(): void
  selection(): string
  paste(text: string): void
  /** Keys as if typed: they come back through `onInput`. */
  type(keys: string): void
  apply(settings: Settings, colors: TerminalColors): void
  dispose(): void
}

export interface TerminalViewOptions {
  settings: Settings
  colors: TerminalColors
  onInput(data: string): void
  onLink(event: MouseEvent, url: string): void
}

export type MakeTerminalView = (options: TerminalViewOptions) => TerminalView
