import { describe, expect, it } from 'vitest'

import { type Area, type Hole, type WallConfig, closed, importsOf, unlisted, violationsIn } from './check'

const WINDOW: Area = {
  name: 'window',
  folders: ['src/window/'],
  mayImport: ['protocol', 'rules'],
  modules: ['react', 'react-dom', '@xterm/']
}

const OTHER_AREAS: Area[] = [
  { name: 'protocol', folders: ['src/protocol/'], mayImport: [], modules: [] },
  { name: 'rules', folders: ['src/rules/'], mayImport: ['protocol', 'rules/transitions'], modules: [] },
  { name: 'rules/transitions', folders: ['src/rules/transitions/'], mayImport: ['protocol', 'rules'], modules: [] },
  {
    name: 'workspace',
    folders: ['src/workspace/'],
    mayImport: ['protocol', 'rules', 'rules/transitions'],
    modules: ['node:']
  },
  { name: 'app', folders: ['src/app/'], mayImport: ['protocol', 'rules', 'workspace'], modules: ['node:'] },
  {
    name: 'app/host',
    folders: ['src/app/host/'],
    mayImport: ['app', 'protocol', 'rules', 'workspace'],
    modules: ['node:', 'electron']
  },
  { name: 'testing', folders: ['src/testing/'], mayImport: [], modules: ['node:'] }
]

const WIDE_WINDOW: Area = { ...WINDOW, mayImport: [...WINDOW.mayImport, 'workspace'] }

const WALLS: WallConfig = {
  root: 'src/',
  aliases: {
    '@protocol/': 'src/protocol/',
    '@rules/': 'src/rules/',
    '@workspace/': 'src/workspace/',
    '@app/': 'src/app/',
    '@window/': 'src/window/',
    '@testing/': 'src/testing/'
  },
  areas: [...OTHER_AREAS, WINDOW],
  testAreas: ['testing'],
  testModules: ['vitest', 'node:'],
  assetExtensions: ['.svg', '.css', '.png']
}

const WIDE_WALLS: WallConfig = { ...WALLS, areas: [...OTHER_AREAS, WIDE_WINDOW] }

function reported(file: string, specifier: string, config: WallConfig = WALLS): string[] {
  const text = `import { thing } from '${specifier}'`
  const files = new Map([[file, text]])
  const violations = violationsIn(files, config)
  return violations.map((violation) => violation.imports)
}

describe('importsOf', () => {
  it('finds every way a file can import', () => {
    const source = [
      "import { a } from 'single'",
      'import {',
      '  b,',
      '  c,',
      '  d',
      "} from 'multi'",
      "import type { T } from 'types'",
      "export { a } from 'reexport'",
      "export * from 'star'",
      "import './app.css'",
      "const lazy = await import('lazy')",
      "const old = require('old')"
    ].join('\n')

    expect(importsOf(source)).toEqual([
      { imports: 'single', line: 1 },
      { imports: 'multi', line: 6 },
      { imports: 'types', line: 7 },
      { imports: 'reexport', line: 8 },
      { imports: 'star', line: 9 },
      { imports: './app.css', line: 10 },
      { imports: 'lazy', line: 11 },
      { imports: 'old', line: 12 }
    ])

    const computed = 'const lazy = await import(name)'
    expect(importsOf(computed)).toEqual([{ imports: '', line: 1 }])

    const files = new Map([['src/window/a.ts', computed]])
    const reasons = violationsIn(files, WALLS).map((violation) => violation.reason)
    expect(reasons).toEqual(['dynamic import is not a string literal'])
  })
})

describe('violationsIn', () => {
  it('a file may import its own area and the areas its row lists', () => {
    const cases: [string, string][] = [
      ['src/window/a.ts', '@protocol/ipc'],
      ['src/window/a.ts', '@rules/drop'],
      ['src/rules/a.ts', './transitions/applyDrop'],
      ['src/workspace/a.ts', '@rules/transitions/applyDrop'],
      ['src/window/a.ts', './sibling'],
      ['src/window/a.ts', '../../branding/mark.svg']
    ]

    for (const [file, specifier] of cases) {
      expect(reported(file, specifier), `${file} imports ${specifier}`).toEqual([])
    }
  })

  it('a crossing is reported with its file, line and specifier', () => {
    const text = ["import { a } from '@protocol/ipc'", "import type { S } from '@workspace/workspaceState'"].join('\n')
    const files = new Map([['src/window/a.ts', text]])
    const violations = violationsIn(files, WALLS)
    const found = violations.map(({ file, line, imports }) => ({ file, line, imports }))
    expect(found).toEqual([{ file: 'src/window/a.ts', line: 2, imports: '@workspace/workspaceState' }])

    const crossings: [string, string][] = [
      ['src/window/a.ts', '@workspace/workspaceState'],
      ['src/window/a.ts', '@rules/transitions/applyDrop'],
      ['src/rules/a.ts', '@workspace/x'],
      ['src/workspace/a.ts', 'electron'],
      ['src/app/a.ts', 'electron'],
      ['src/window/a.ts', 'node:fs'],
      ['src/protocol/a.ts', '@rules/arrangement']
    ]

    for (const [file, specifier] of crossings) {
      expect(reported(file, specifier), `${file} imports ${specifier}`).toEqual([specifier])
    }

    expect(reported('src/app/host/index.ts', 'electron')).toEqual([])
  })

  it('a relative import may not leave its package', () => {
    const climbing = '../workspace/workspaceState'
    expect(reported('src/window/a.ts', climbing, WIDE_WALLS)).toEqual([climbing])
    expect(reported('src/app/host/index.ts', '../agents')).toEqual([])
  })

  it('a file belongs to the longest folder that holds it, whatever order the areas are listed in', () => {
    const innerFirst: WallConfig = { ...WALLS, areas: [...WALLS.areas].reverse() }
    for (const config of [WALLS, innerFirst]) {
      expect(reported('src/window/a.ts', '@rules/transitions/applyDrop', config)).toEqual([
        '@rules/transitions/applyDrop'
      ])
      expect(reported('src/app/host/index.ts', 'electron', config)).toEqual([])
    }
  })

  it('an alias cannot be walked out of with ..', () => {
    const sideways = '@protocol/../app/service'
    expect(reported('src/window/a.ts', sideways)).toEqual([sideways])
  })

  it('a folder imported for its index file counts as that folder', () => {
    expect(reported('src/app/service.ts', './host')).toEqual(['./host'])
    expect(reported('src/rules/drop.ts', './transitions')).toEqual([])
  })

  it('a test file may also import the testing area and vitest', () => {
    expect(reported('src/workspace/x.test.ts', '@testing/tempDirectory')).toEqual([])
    expect(reported('src/workspace/x.test.ts', 'vitest')).toEqual([])
    expect(reported('src/workspace/x.ts', '@testing/tempDirectory')).toEqual(['@testing/tempDirectory'])
  })

  it('a source file in no area is reported', () => {
    const files = new Map([['src/stray/thing.ts', '']])
    const violations = violationsIn(files, WALLS)
    expect(violations).toEqual([
      { file: 'src/stray/thing.ts', line: 0, imports: '', reason: 'belongs to no area' }
    ])
  })
})

describe('holes', () => {
  it('an unlisted crossing and a closed hole are both caught', () => {
    const sameFileTwice = "import { x } from '@workspace/x'\nimport { y } from '@workspace/y'"
    const files = new Map([
      ['src/rules/a.ts', sameFileTwice],
      ['src/workspace/b.ts', "import { app } from 'electron'"]
    ])
    const violations = violationsIn(files, WALLS)
    const holes: Hole[] = [
      { file: 'src/rules/a.ts', imports: '@workspace/x', closes: 3 },
      { file: 'src/gone.ts', imports: 'electron', closes: 4 }
    ]

    const [, otherImportOfListedFile, otherFile] = violations
    const [, staleHole] = holes
    expect(unlisted(violations, holes)).toEqual([otherImportOfListedFile, otherFile])
    expect(closed(violations, holes)).toEqual([staleHole])
  })
})
