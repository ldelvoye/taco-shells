import { useEffect, useState } from 'react'
import type { Config } from '@shared/config'

export function useConfig(): Config {
  const [config, setConfig] = useState<Config>(window.taqueria.config.initial)

  useEffect(() => {
    return window.taqueria.config.onChange(setConfig)
  }, [])

  return config
}
