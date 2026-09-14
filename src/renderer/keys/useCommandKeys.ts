import { useEffect } from 'react'
import type { CommandId } from '@shared/commands'
import { commandForKeyStroke } from '@shared/keys'
import type { CommandRegistry } from '../commands/registry'

export function useCommandKeys(
  registry: CommandRegistry,
  bindings: ReadonlyMap<string, CommandId>
): void {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      const commandId = commandForKeyStroke(bindings, event)
      if (!commandId) {
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
