import { SerializeAddon } from '@xterm/addon-serialize'
import { Terminal } from '@xterm/headless'

import type { PtySize } from '@protocol/ipc'
import type { TerminalColors } from '@protocol/theme'
import type { MakeScreen, Screen, ScreenOptions } from '@runtime/screen'

import { colorAnswers } from './colors'
import { trackCursorHidden } from './cursor'

// The serialize add-on does not carry whether the cursor is hidden, so a snapshot ends with this when it is.
const HIDE_CURSOR = '\x1b[?25l'
const ICON_TITLE_OSC = 1
const COLOR_SEQUENCES: (4 | 10 | 11 | 12)[] = [4, 10, 11, 12]

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
  const replyListeners: ((data: string) => void)[] = []
  let colors: TerminalColors | null = null

  function reply(data: string): void {
    for (const listener of replyListeners) {
      listener(data)
    }
  }

  term.onData(reply)
  for (const sequence of COLOR_SEQUENCES) {
    term.parser.registerOscHandler(sequence, (data) => {
      if (colors !== null) {
        const answers = colorAnswers(sequence, data, colors)
        for (const answer of answers) {
          reply(answer)
        }
      }
      return false
    })
  }

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
    // Both title sequences count, and the last one a program sets wins. False
    // leaves xterm's own handler to record the icon name.
    onTitle(listener: (title: string) => void): void {
      term.onTitleChange(listener)
      term.parser.registerOscHandler(ICON_TITLE_OSC, (title) => {
        listener(title)
        return false
      })
    },
    onReply(listener: (data: string) => void): void {
      replyListeners.push(listener)
    },
    setColors(next: TerminalColors): void {
      colors = next
    },
    dispose(): void {
      term.dispose()
    }
  }
}
