import { type JSX, useEffect, useRef } from 'react'
import type { SessionId } from '@protocol/ipc'
import { attachSession, detachSession } from './registry'

interface TerminalHostProps {
  sessionId: SessionId
  width: number
}

// Sessions in a hidden group stay mounted rather than unmounting. An xterm
// element changes host only when its pane moves to another split.
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
