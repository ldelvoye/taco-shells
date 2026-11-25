const SESSION_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** Whether a value is a Claude Code session id (a UUID string). */
export function isClaudeSessionId(value: unknown): value is string {
  if (typeof value !== 'string') {
    return false
  }
  return SESSION_UUID.test(value)
}
