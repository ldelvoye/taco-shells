import { type JSX, useEffect, useRef } from 'react'
import type { SessionId } from '@protocol/ipc'
import { attachSession, detachSession } from './registry'

interface TerminalHostProps {
  sessionId: SessionId
  width: number
}

// A pane in a hidden group stays mounted, so its terminal keeps a host to be
// sized by. Whether that host holds a view is the registry's business.
export function TerminalHost({ sessionId, width }: TerminalHostProps): JSX.Element {
  const hostRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) {
      return
    }

    attachSession(sessionId, host)
    return () => {
      detachSession(sessionId)
    }
  }, [sessionId])

  return <div className="terminal-view" ref={hostRef} style={{ flexGrow: width }} />
}
