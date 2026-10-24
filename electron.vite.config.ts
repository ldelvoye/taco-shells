import { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'electron-vite'

const sharedDir = resolve(__dirname, 'src/shared')

// Baked in so the running app knows which config directory is its own, and which
// sidebar mark to draw, matching the identity electron-builder brands the bundle
// with.
let channel: string
if (process.env.TACO_SHELLS_CHANNEL === 'stable') {
  channel = 'stable'
} else {
  channel = 'development'
}

export default defineConfig({
  main: {
    resolve: { alias: { '@shared': sharedDir } },
    define: { __CHANNEL__: JSON.stringify(channel) }
  },
  preload: {
    resolve: { alias: { '@shared': sharedDir } }
  },
  renderer: {
    plugins: [react()],
    resolve: { alias: { '@shared': sharedDir } },
    define: { __CHANNEL__: JSON.stringify(channel) }
  }
})
