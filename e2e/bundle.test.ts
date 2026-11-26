import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// `npm run e2e` builds first, without TACO_SHELLS_CHANNEL, so out/ is a dev build.
const MAIN_BUNDLE = resolve(import.meta.dirname, '../out/main/index.js')
const RUNTIME_BUNDLE = resolve(import.meta.dirname, '../out/main/runtime.js')

const REQUIRE = /require\(["']([^"']+)["']\)/g
const NODE_BUILT_IN = 'node:'
// The terminal process has no `electron` module to load, and no page for the
// browser build of xterm: requiring either there fails at startup.
const RUNTIME_PACKAGES = ['@xterm/addon-serialize', '@xterm/headless', 'node-pty']

// Quoted, because `.taco-shells` is a prefix of `.taco-shells-dev` and the stable
// name appears inside the dev name and inside user-facing messages.
const STABLE_DIRECTORY_LITERAL = /["'`]\.taco-shells["'`]/
const STABLE_NAME_LITERAL = /["'`]Taco Shells["'`]/
const DEVELOPMENT_DIRECTORY_LITERAL = /["'`]\.taco-shells-dev["'`]/

describe('the dev build cannot reach the stable config', () => {
  it('bakes only the dev channel into the main bundle', () => {
    const bundle = readFileSync(MAIN_BUNDLE, 'utf8')

    expect(bundle).toMatch(DEVELOPMENT_DIRECTORY_LITERAL)
    expect(bundle).not.toMatch(STABLE_DIRECTORY_LITERAL)
    expect(bundle).not.toMatch(STABLE_NAME_LITERAL)
  })
})

describe('the terminal process stands alone', () => {
  it('its bundle loads the shells and the headless terminal, and nothing else from outside Node', () => {
    const bundle = readFileSync(RUNTIME_BUNDLE, 'utf8')
    const matches = [...bundle.matchAll(REQUIRE)]
    const required = matches.map((match) => match[1])
    const packages = required.filter((name) => !name.startsWith(NODE_BUILT_IN))
    const distinct = new Set(packages)
    const sorted = [...distinct].sort()

    expect(sorted).toEqual(RUNTIME_PACKAGES)
  })
})
