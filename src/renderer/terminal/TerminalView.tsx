import { type JSX, useEffect, useRef } from 'react'
import type { SessionId } from '@shared/ipc'
import { attachSession, detachSession } from './registry'

interface TerminalViewProps {
  sessionId: SessionId
  active: boolean
}

// Inactive sessions stay mounted rather than unmounting, so an xterm element is
// never reparented from one host to another.
export function TerminalView({ sessionId, active }: TerminalViewProps): JSX.Element {
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

  let className = 'terminal-view'
  if (!active) {
    className = 'terminal-view is-inactive'
  }

  return <div className={className} ref={hostRef} />
}
