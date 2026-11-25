export interface Deadline {
  signal: AbortSignal
  clear(): void
}

/** A signal that aborts once a delay has passed, unless it is cleared first. */
export function deadlineAfter(delayMs: number): Deadline {
  const controller = new AbortController()
  const timer = setTimeout(() => {
    controller.abort()
  }, delayMs)

  return {
    signal: controller.signal,
    clear: () => {
      clearTimeout(timer)
    }
  }
}
