import react from '@vitejs/plugin-react'
import { defineConfig } from 'electron-vite'

// Baked in so the running app knows which config directory is its own, and which
// sidebar mark to draw, matching the identity electron-builder brands the bundle
// with.
let channel: string
if (process.env.TACO_SHELLS_CHANNEL === 'stable') {
  channel = 'stable'
} else {
  channel = 'development'
}

// `@shared` resolves through the tsconfig whose `include` covers each file:
// tsconfig.node.json for main and preload, tsconfig.web.json for the renderer.
export default defineConfig({
  main: {
    resolve: { tsconfigPaths: true },
    define: { __CHANNEL__: JSON.stringify(channel) }
  },
  preload: {
    resolve: { tsconfigPaths: true }
  },
  renderer: {
    plugins: [react()],
    resolve: { tsconfigPaths: true },
    define: { __CHANNEL__: JSON.stringify(channel) }
  }
})
