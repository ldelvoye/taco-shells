import { resolve } from 'node:path'

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

// The package aliases resolve through the tsconfig whose `include` covers each file:
// tsconfig.node.json for main and preload, tsconfig.web.json for the renderer.
export default defineConfig({
  main: {
    resolve: { tsconfigPaths: true },
    build: {
      lib: {
        entry: {
          index: resolve('src/app/host/index.ts'),
          runtime: resolve('src/runtime/main.ts')
        }
      }
    },
    define: { __CHANNEL__: JSON.stringify(channel) }
  },
  preload: {
    resolve: { tsconfigPaths: true },
    build: { lib: { entry: resolve('src/app/preload/index.ts') } }
  },
  renderer: {
    root: 'src/window',
    plugins: [react()],
    resolve: { tsconfigPaths: true },
    build: { rolldownOptions: { input: resolve('src/window/index.html') } },
    define: { __CHANNEL__: JSON.stringify(channel) }
  }
})
