import { defineConfig } from 'vitest/config'

// Each test launches and quits the real app, so these are slow, serial, and kept
// out of `npm test`. `npm run test:e2e` builds first: driving a stale `out/`
// passes while the change under test is not in it.
export default defineConfig({
  test: {
    environment: 'node',
    include: ['e2e/**/*.test.ts'],
    testTimeout: 60000,
    hookTimeout: 60000,
    fileParallelism: false
  }
})
