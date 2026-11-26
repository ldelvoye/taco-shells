import { serve } from '@hono/node-server'

import { createApp } from './app.ts'

const DEFAULT_PORT = 8080

function readPort(): number {
  const fromEnv = process.env.PORT
  if (fromEnv === undefined || fromEnv === '') {
    return DEFAULT_PORT
  }
  return Number(fromEnv)
}

const port = readPort()
const app = createApp()

serve({ fetch: app.fetch, port }, (info) => {
  console.log(`Taco Shells site on http://localhost:${info.port}`)
})
