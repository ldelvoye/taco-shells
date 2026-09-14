import { type JSX, useEffect, useRef } from 'react'
import type { SessionId } from '@shared/ipc'
import { attachSession, detachSession } from './registry'

interface TerminalViewProps {
  sessionId: SessionId
}

export function TerminalView({ sessionId }: TerminalViewProps): JSX.Element {
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

  return <div className="terminal-view" ref={hostRef} />
}
