import { ipcMain } from 'electron'
import { CHANNEL } from '@protocol/ipc'
import type { Snapshot } from '@protocol/snapshot'
import type { WorkspaceRequest } from '@protocol/workspace'
import type { SnapshotStore } from '@workspace/snapshot/store'
import { Workspace } from '@workspace/workspace'
import type { WindowLink } from '../window/link'

export function bridgeWorkspace(
  link: WindowLink,
  snapshots: SnapshotStore,
  restoreOnLaunch: boolean
): { workspace: Workspace; stop(): void } {
  let snapshot: Snapshot | null = null
  if (restoreOnLaunch) {
    snapshot = snapshots.initial()
  }

  const workspace = new Workspace(snapshot, {
    onState: (state) => {
      link.send(CHANNEL.workspaceState, state)
    },
    onArrangement: (arrangement) => {
      snapshots.workspaceChanged(arrangement)
    },
    // The terminals are the window's only contents, so losing the last of them is
    // the same gesture as closing the window.
    onEmptied: () => {
      link.close()
    }
  })

  // sendSync is answered by assigning returnValue, not by returning: a handler
  // that returns instead leaves the renderer waiting at startup.
  ipcMain.on(CHANNEL.workspaceGet, (event) => {
    event.returnValue = workspace.drawn()
  })
  ipcMain.handle(CHANNEL.workspaceRequest, (_event, request: WorkspaceRequest) => {
    return workspace.request(request)
  })

  return {
    workspace,
    stop: () => {
      ipcMain.removeAllListeners(CHANNEL.workspaceGet)
      ipcMain.removeHandler(CHANNEL.workspaceRequest)
    }
  }
}
