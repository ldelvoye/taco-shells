import { join } from 'node:path'

import { serveStatic } from '@hono/node-server/serve-static'
import { Hono } from 'hono'

export const INSTALL_SCRIPT_URL = 'https://github.com/ldelvoye/taco-shells/releases/latest/download/install.sh'

const PUBLIC_DIR = join(import.meta.dirname, '..', 'public')

export function createApp(): Hono {
  const app = new Hono()

  app.get('/install.sh', (c) => c.redirect(INSTALL_SCRIPT_URL, 302))
  app.get('/health', (c) => c.text('ok'))
  app.use('*', serveStatic({ root: PUBLIC_DIR }))

  return app
}
