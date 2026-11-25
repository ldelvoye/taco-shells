import { useEffect, useState } from 'react'

interface UpdateState {
  ready: string | null
  problem: string | null
}

/** The version of a downloaded update, if any, and the reason applying one failed. */
export function useUpdate(): UpdateState {
  const [ready, setReady] = useState<string | null>(window.tacoShells.update.initial)
  const [problem, setProblem] = useState<string | null>(null)

  useEffect(() => {
    return window.tacoShells.update.onReady(setReady)
  }, [])

  useEffect(() => {
    return window.tacoShells.update.onProblem(setProblem)
  }, [])

  return { ready, problem }
}
