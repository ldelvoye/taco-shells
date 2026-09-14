import type { TaqueriaApi } from '@shared/ipc'

declare global {
  interface Window {
    taqueria: TaqueriaApi
  }
}
