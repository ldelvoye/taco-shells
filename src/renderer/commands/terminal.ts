import type { SessionId } from '@shared/ipc'
import { sessionTerminal } from '../terminal/registry'
import type { CommandRegistry } from './registry'

// What ctrl+U deletes is the reading program's call rather than ours: back to the
// line start under readline, the whole line under zsh, half a page under vim.
const CTRL_U = '\x15'

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

  registry.register('terminal.killLine', () => {
    const term = sessionTerminal(sessionId)
    if (!term) {
      return
    }

    term.input(CTRL_U)
  })
}
