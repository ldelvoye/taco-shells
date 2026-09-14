import { useEffect, useState } from 'react'
import type { Config } from '@shared/config'

export function useConfig(): Config {
  const [config, setConfig] = useState<Config>(window.tacoShells.config.initial)

  useEffect(() => {
    return window.tacoShells.config.onChange(setConfig)
  }, [])

  return config
}
