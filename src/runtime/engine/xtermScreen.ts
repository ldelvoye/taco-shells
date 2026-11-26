import { SerializeAddon } from '@xterm/addon-serialize'
import { Terminal } from '@xterm/headless'

import type { PtySize } from '@protocol/ipc'
import type { MakeScreen, Screen, ScreenOptions } from '@runtime/screen'

import { trackCursorHidden } from './cursor'

const HIDE_CURSOR = '\x1b[?25l'
const ICON_TITLE_OSC = 1

export const makeXtermScreen: MakeScreen = (options: ScreenOptions): Screen => {
  const term = new Terminal({
    cols: options.size.cols,
    rows: options.size.rows,
    scrollback: options.scrollback,
    allowProposedApi: true
  })
  const serialize = new SerializeAddon()
  term.loadAddon(serialize)
  const cursorHidden = trackCursorHidden(term)

  return {
    write(data: string, parsed: () => void): void {
      term.write(data, parsed)
    },
    afterParsed(then: () => void): void {
      term.write('', then)
    },
    resize(size: PtySize): void {
      term.resize(size.cols, size.rows)
    },
    clear(): void {
      term.clear()
    },
    setScrollback(lines: number): void {
      term.options.scrollback = lines
    },
    snapshot(): string {
      const screen = serialize.serialize()
      if (cursorHidden()) {
        return screen + HIDE_CURSOR
      }
      return screen
    },
    onTitle(listener: (title: string) => void): void {
      term.onTitleChange(listener)
      term.parser.registerOscHandler(ICON_TITLE_OSC, (title) => {
        listener(title)
        return false
      })
    },
    onReply(listener: (data: string) => void): void {
      term.onData(listener)
    },
    dispose(): void {
      term.dispose()
    }
  }
}
