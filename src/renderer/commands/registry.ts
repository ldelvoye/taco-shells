import type { CommandId } from '@shared/commands'

export type CommandHandler = () => void | Promise<void>

export interface CommandRegistry {
  register(id: CommandId, handler: CommandHandler): void
  has(id: CommandId): boolean
  run(id: CommandId): void
}

export function createCommandRegistry(): CommandRegistry {
  const handlers = new Map<CommandId, CommandHandler>()

  return {
    register(id, handler) {
      handlers.set(id, handler)
    },

    has(id) {
      return handlers.has(id)
    },

    run(id) {
      const handler = handlers.get(id)
      if (!handler) {
        console.warn(`no handler registered for command ${id}`)
        return
      }

      const result = handler()
      void Promise.resolve(result).catch((error: unknown) => {
        console.error(`command ${id} failed`, error)
      })
    }
  }
}
