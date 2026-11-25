import type { WallConfig } from './check'

// What each part of the app may import. A line added here widens a wall for good: prefer a hole in holes.ts, which has to name the milestone that closes it.
// An area is a package's folder or a part of one, and a file belongs to the area with the longest folder that holds it. app and window have no alias: nothing outside them may import them.
export const WALLS: WallConfig = {
  root: 'src/',
  aliases: {
    '@protocol/': 'src/protocol/',
    '@rules/': 'src/rules/',
    '@settings/': 'src/settings/',
    '@agents/': 'src/agents/',
    '@runtime/': 'src/runtime/',
    '@workspace/': 'src/workspace/',
    '@testing/': 'src/testing/'
  },
  areas: [
    { name: 'protocol', folders: ['src/protocol/'], mayImport: [], modules: [] },
    {
      name: 'rules',
      folders: ['src/rules/'],
      mayImport: ['protocol', 'rules/transitions'],
      modules: []
    },
    {
      name: 'rules/transitions',
      folders: ['src/rules/transitions/'],
      mayImport: ['protocol', 'rules'],
      modules: []
    },
    { name: 'settings', folders: ['src/settings/'], mayImport: ['protocol'], modules: [] },
    { name: 'agents', folders: ['src/agents/'], mayImport: ['protocol'], modules: [] },
    {
      name: 'runtime',
      folders: ['src/runtime/'],
      mayImport: ['protocol'],
      modules: ['node:', 'node-pty']
    },
    {
      name: 'workspace',
      folders: ['src/workspace/'],
      mayImport: ['protocol', 'rules', 'rules/transitions', 'agents'],
      modules: ['node:']
    },
    {
      name: 'app',
      folders: ['src/app/'],
      mayImport: ['protocol', 'rules', 'settings', 'agents', 'workspace'],
      modules: ['node:']
    },
    {
      name: 'app/services',
      folders: ['src/app/agents/', 'src/app/config/', 'src/app/focus/', 'src/app/update/'],
      mayImport: ['app', 'protocol', 'rules', 'settings', 'agents', 'workspace'],
      modules: ['node:', 'jsonc-parser']
    },
    {
      name: 'app/host',
      folders: ['src/app/host/'],
      mayImport: ['app', 'app/services', 'protocol', 'rules', 'settings', 'agents', 'workspace'],
      modules: ['node:', 'electron']
    },
    {
      name: 'app/preload',
      folders: ['src/app/preload/'],
      mayImport: ['protocol'],
      modules: ['electron']
    },
    {
      name: 'window',
      folders: ['src/window/'],
      mayImport: ['protocol', 'rules', 'settings'],
      modules: ['react', 'react-dom', '@xterm/']
    },
    { name: 'testing', folders: ['src/testing/'], mayImport: [], modules: ['node:'] }
  ],
  testAreas: ['testing'],
  testModules: ['vitest', 'node:'],
  assetExtensions: ['.svg', '.css', '.png']
}
