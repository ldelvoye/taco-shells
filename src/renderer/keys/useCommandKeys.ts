import { useEffect } from 'react'
import { REPEATING_COMMANDS, type CommandId } from '@shared/commands'
import { commandForKeyStroke } from '@shared/keys'
import type { CommandRegistry } from '../commands/registry'

export function useCommandKeys(
  registry: CommandRegistry,
  bindings: ReadonlyMap<string, CommandId>
): void {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      // A text field keeps its keys: cmd+w must not close a terminal while a name is being typed.
      if (event.target instanceof HTMLInputElement) {
        return
      }

      const commandId = commandForKeyStroke(bindings, event)
      if (!commandId) {
        return
      }

      // Most commands are discrete actions, so a held key must fire them once
      // rather than at the keyboard's repeat rate: holding cmd+t alone would
      // spawn shells for as long as it stayed down.
      const repeatsWhileHeld = REPEATING_COMMANDS.has(commandId)
      if (event.repeat && !repeatsWhileHeld) {
        return
      }
      if (!registry.has(commandId)) {
        return
      }

      event.preventDefault()
      event.stopPropagation()
      registry.run(commandId)
    }

    // Capture phase: xterm listens on its own textarea, so a bubbling listener
    // would only see the keys the terminal decided not to keep.
    window.addEventListener('keydown', onKeyDown, true)
    return () => {
      window.removeEventListener('keydown', onKeyDown, true)
    }
  }, [registry, bindings])
}
