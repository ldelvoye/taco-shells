import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// `npm run e2e` builds first, without TACO_SHELLS_CHANNEL, so out/ is a dev build.
const MAIN_BUNDLE = resolve(import.meta.dirname, '../out/main/index.js')

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
