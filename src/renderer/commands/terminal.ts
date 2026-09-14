import type { SessionId } from '@shared/ipc'
import { sessionTerminal } from '../terminal/registry'
import type { CommandRegistry } from './registry'

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
}
