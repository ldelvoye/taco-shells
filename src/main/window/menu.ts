import { app, Menu, type MenuItemConstructorOptions } from 'electron'

export interface ReadyUpdate {
  version: string
  onApply: () => void
}

// No Edit menu: its copy and paste roles would claim cmd+c and cmd+v, and a menu
// accelerator fires before the renderer sees the keydown.
// No Close item: its cmd+w has to stay free for closing a terminal.
export function buildApplicationMenu(readyUpdate: ReadyUpdate | null = null): Menu {
  const template: MenuItemConstructorOptions[] = [
    {
      label: app.name,
      submenu: [
        { role: 'about' },
        { type: 'separator' },
        { role: 'hide' },
        { role: 'hideOthers' },
        { role: 'unhide' },
        { type: 'separator' },
        { role: 'quit' }
      ]
    },
    {
      label: 'View',
      submenu: [{ role: 'toggleDevTools' }]
    },
    {
      label: 'Window',
      submenu: [{ role: 'minimize' }, { role: 'zoom' }]
    }
  ]

  if (readyUpdate !== null) {
    template.push({
      label: 'Update',
      submenu: [
        {
          label: `Restart to Update (${readyUpdate.version})`,
          click: readyUpdate.onApply
        }
      ]
    })
  }

  return Menu.buildFromTemplate(template)
}
