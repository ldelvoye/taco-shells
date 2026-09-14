import { app, Menu } from 'electron'
import { registerIpc } from './ipc'
import { PtySessions } from './pty/sessions'
import { buildApplicationMenu } from './window/menu'
import { createMainWindow } from './window/window'

app.setName('Taqueria')

const sessions = new PtySessions()

app.whenReady().then(() => {
  Menu.setApplicationMenu(buildApplicationMenu())
  const window = createMainWindow()
  registerIpc(sessions, window)
})

app.on('before-quit', () => {
  sessions.killAll()
})

// A terminal manager with no windows has nothing left to manage, so closing the
// window quits rather than leaving the app resident the way macOS usually does.
app.on('window-all-closed', () => {
  app.quit()
})
