import { nativeTheme } from 'electron'
import type { SystemTheme } from '../config'

export function nativeSystemTheme(): SystemTheme {
  return {
    prefersDark: () => nativeTheme.shouldUseDarkColors,
    onChange: (listener: () => void) => {
      nativeTheme.on('updated', listener)
      return () => {
        nativeTheme.off('updated', listener)
      }
    }
  }
}
