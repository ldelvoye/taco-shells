import { resolve } from 'node:path'
import react from '@vitejs/plugin-react'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'

const sharedDir = resolve(__dirname, 'src/shared')
const rendererDir = resolve(__dirname, 'src/renderer')

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    resolve: { alias: { '@shared': sharedDir } },
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
