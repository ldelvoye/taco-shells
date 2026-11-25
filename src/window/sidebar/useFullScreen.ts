import { useEffect, useState } from 'react'

export function useFullScreen(): boolean {
  const [fullScreen, setFullScreen] = useState(window.tacoShells.fullScreen.initial)

  useEffect(() => {
    return window.tacoShells.fullScreen.onChange(setFullScreen)
  }, [])

  return fullScreen
}
