import type { TacoShellsApi } from '@shared/ipc'

declare global {
  interface Window {
    tacoShells: TacoShellsApi
  }
}
