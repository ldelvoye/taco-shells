import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: { tsconfigPaths: true },
  // The main process reads its channel when a module loads, so tests run as the
  // dev build unless they stub the global themselves.
  define: { __CHANNEL__: JSON.stringify('development') },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'scripts/**/*.test.mjs']
  }
})
