import { app, Menu, type MenuItemConstructorOptions } from 'electron'

// No Edit menu: its copy and paste roles would claim cmd+c and cmd+v, and a menu
// accelerator fires before the renderer sees the keydown.
// No Close item: its cmd+w has to stay free for closing a terminal.
export function buildApplicationMenu(): Menu {
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

  return Menu.buildFromTemplate(template)
}
