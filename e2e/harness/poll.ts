export const POLL_INTERVAL_MS = 100

export function sleep(milliseconds: number): Promise<void> {
  return new Promise((done) => setTimeout(done, milliseconds))
}

/**
 * Retries `attempt` until it answers with something other than null, or the
 * timeout runs out. Polling rather than sleeping a guessed interval is what keeps
 * these tests from being a pile of timeouts.
 */
export async function pollFor<T>(
  description: string,
  attempt: () => Promise<T | null>,
  timeoutMs: number
): Promise<T> {
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    const found = await attempt()
    if (found !== null) {
      return found
    }
    await sleep(POLL_INTERVAL_MS)
  }

  throw new Error(`timed out waiting for ${description}`)
}
