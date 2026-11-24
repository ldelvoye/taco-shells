import type { CommandId } from '@shared/commands'
import type { SessionId } from '@shared/ipc'
import { sessionTerminal } from '../terminal/registry'
import type { CommandRegistry } from './registry'

// A terminal's usual key for delete line.
const CTRL_U = '\x15'

// A terminal's usual keys for start and end of line.
const CTRL_A = '\x01'
const CTRL_E = '\x05'

// A terminal's usual keys for back and forward a word: option+b and option+f.
const ESC_B = '\x1bb'
const ESC_F = '\x1bf'

// A terminal's usual key for a newline that does not submit the line: option+enter.
const ESC_CR = '\x1b\r'

const LINE_EDITING_KEYS: ReadonlyArray<readonly [CommandId, string]> = [
  ['terminal.killLine', CTRL_U],
  ['terminal.lineStart', CTRL_A],
  ['terminal.lineEnd', CTRL_E],
  ['terminal.wordLeft', ESC_B],
  ['terminal.wordRight', ESC_F],
  ['terminal.insertNewline', ESC_CR]
]

export function registerTerminalCommands(registry: CommandRegistry, sessionId: SessionId): void {
  registry.register('terminal.copy', () => {
    const term = sessionTerminal(sessionId)
    if (!term) {
      return
    }

    const selection = term.getSelection()
    if (!selection) {
      return
    }

    window.tacoShells.clipboard.write(selection)
  })

  // Pasting through xterm rather than straight down the pty: it is what wraps the
  // text in the bracketed-paste markers, without which the shell runs every line
  // of a multi-line paste the moment it arrives.
  registry.register('terminal.paste', async () => {
    const term = sessionTerminal(sessionId)
    if (!term) {
      return
    }

    const text = await window.tacoShells.clipboard.read()
    if (!text) {
      return
    }

    term.paste(text)
  })

  registry.register('terminal.clear', () => {
    const term = sessionTerminal(sessionId)
    if (!term) {
      return
    }

    term.clear()
  })

  for (const [commandId, keys] of LINE_EDITING_KEYS) {
    registry.register(commandId, () => {
      const term = sessionTerminal(sessionId)
      if (!term) {
        return
      }

      term.input(keys)
    })
  }
}
