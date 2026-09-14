import { type JSX, useEffect, useMemo, useState } from 'react'
import type { SessionId } from '@shared/ipc'
import { createCommandRegistry } from './commands/registry'
import { registerTerminalCommands } from './commands/terminal'
import { keyBindings } from './keys/bindings'
import { useCommandKeys } from './keys/useCommandKeys'
import { Sidebar } from './sidebar/Sidebar'
import { createSession, disposeSession } from './terminal/registry'
import { TerminalView } from './terminal/TerminalView'

export function App(): JSX.Element {
  const [sessionId, setSessionId] = useState<SessionId | null>(null)
  const commands = useMemo(() => createCommandRegistry(), [])

  useEffect(() => {
    let cancelled = false
    let created: SessionId | null = null

    void createSession().then((id) => {
      if (cancelled) {
        disposeSession(id)
        return
      }
      created = id
      setSessionId(id)
    })

    return () => {
      cancelled = true
      if (created) {
        disposeSession(created)
      }
    }
  }, [])

  useEffect(() => {
    if (!sessionId) {
      return
    }
    registerTerminalCommands(commands, sessionId)
  }, [commands, sessionId])

  // The shell is the only thing in the window, so `exit` closing it is the same
  // gesture as closing the window.
  useEffect(() => {
    if (!sessionId) {
      return
    }

    return window.taqueria.pty.onExit((event) => {
      if (event.id !== sessionId) {
        return
      }
      window.close()
    })
  }, [sessionId])

  useCommandKeys(commands, keyBindings)

  let terminal = null
  if (sessionId) {
    terminal = <TerminalView sessionId={sessionId} />
  }

  return (
    <div className="app">
      <Sidebar />
      <main className="pane-area">{terminal}</main>
    </div>
  )
}
