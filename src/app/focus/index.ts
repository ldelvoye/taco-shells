import type { Runtime } from '@protocol/runtime'
import { focusPortPath } from '../config/files'
import type { Service } from '../service'
import { startFocusServer, type FocusServerHandlers } from './server'

export type FocusEvents = Omit<FocusServerHandlers, 'sessionForPids' | 'onUpdateCheck'>

export function startFocus(
  runtime: Runtime,
  directory: string,
  events: FocusEvents,
  onUpdateCheck: () => void
): Service {
  const portFilePath = focusPortPath(directory)

  const handlers: FocusServerHandlers = {
    ...events,
    sessionForPids: (pids: number[]) => runtime.terminalForPids(pids),
    onUpdateCheck
  }
  const stopServer = startFocusServer(handlers, portFilePath)

  return {
    stop: () => {
      stopServer()
    }
  }
}
