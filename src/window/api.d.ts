import type { TacoShellsApi } from '@protocol/ipc'

declare global {
  interface Window {
    tacoShells: TacoShellsApi
  }
}
