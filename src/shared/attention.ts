import type { SessionId } from './ipc'

/** The pane a raise should jump to once a session asks for attention. */
export function waitingAfterAttention(
  current: SessionId | null,
  asking: SessionId,
  active: SessionId | null
): SessionId | null {
  // Asking while you are already in that pane is not a request to go anywhere.
  if (asking === active) {
    return current
  }
  return asking
}

/** The pane a raise should jump to once the active pane changes. */
export function waitingAfterFocus(
  current: SessionId | null,
  active: SessionId | null
): SessionId | null {
  if (current === active) {
    return null
  }
  return current
}

/** The pane a raise should jump to once a session's shell exits. */
export function waitingAfterExit(current: SessionId | null, exited: SessionId): SessionId | null {
  if (current === exited) {
    return null
  }
  return current
}
