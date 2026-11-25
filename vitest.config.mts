import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: { tsconfigPaths: true },
  // app/channel.ts reads __CHANNEL__ at load, so tests run as the dev build unless they stub it.
  define: { __CHANNEL__: JSON.stringify('development') },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'walls/**/*.test.ts', 'scripts/**/*.test.mjs']
  }
})
