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

// The `@shared` alias comes from the tsconfig `paths`, so it is declared once.
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
