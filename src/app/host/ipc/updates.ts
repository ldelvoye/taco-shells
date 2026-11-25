import { ipcMain } from 'electron'
import { CHANNEL } from '@protocol/ipc'
import type { UpdateEvents } from '../../update/checker'
import type { UpdateService } from '../../update'
import type { ApplicationMenu } from '../window/applicationMenu'
import type { WindowLink } from '../window/link'

export function bridgeUpdates(
  link: WindowLink,
  menu: ApplicationMenu,
  start: (events: UpdateEvents) => UpdateService
): UpdateService {
  const events: UpdateEvents = {
    onReady: (version: string) => {
      link.send(CHANNEL.updateReady, version)
      menu.setReadyUpdate({
        version,
        onApply: () => {
          updates.applyReady()
        }
      })
    },
    onProblem: (message: string) => {
      link.send(CHANNEL.updateProblem, message)
    }
  }
  const updates = start(events)

  // A sendSync left unanswered hangs the renderer at startup, so this answers
  // in a build that never checks for updates too.
  ipcMain.on(CHANNEL.updateGet, (event) => {
    event.returnValue = updates.readyVersion()
  })

  ipcMain.on(CHANNEL.updateApply, () => {
    updates.applyReady()
  })

  return {
    ...updates,
    stop: () => {
      updates.stop()
      ipcMain.removeAllListeners(CHANNEL.updateApply)
      ipcMain.removeAllListeners(CHANNEL.updateGet)
    }
  }
}
