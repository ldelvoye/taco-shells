import { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'

const sharedDir = resolve(__dirname, 'src/shared')
const rendererDir = resolve(__dirname, 'src/renderer')

// Baked in so the running app knows which config directory is its own, matching
// the identity electron-builder brands the bundle with.
let channel: string
if (process.env.TACO_SHELLS_CHANNEL === 'stable') {
  channel = 'stable'
} else {
  channel = 'development'
}

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    resolve: { alias: { '@shared': sharedDir } },
    define: { __CHANNEL__: JSON.stringify(channel) },
    build: { rollupOptions: { input: resolve(__dirname, 'src/main/index.ts') } }
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    resolve: { alias: { '@shared': sharedDir } },
    build: { rollupOptions: { input: resolve(__dirname, 'src/preload/index.ts') } }
  },
  renderer: {
    root: rendererDir,
    plugins: [react()],
    resolve: { alias: { '@shared': sharedDir, '@renderer': rendererDir } },
    build: { rollupOptions: { input: resolve(rendererDir, 'index.html') } }
  }
})
