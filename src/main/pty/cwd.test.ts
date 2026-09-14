import { describe, expect, it } from 'vitest'

import { parseCwdOutput } from './cwd'

describe('parseCwdOutput', () => {
  it('extracts the path from the n line, and falls back to null when there is none', () => {
    const realLsofOutput = 'p4345\nfcwd\nn/Users/lucasdelvoye/code/taqueria\n'
    expect(parseCwdOutput(realLsofOutput)).toBe('/Users/lucasdelvoye/code/taqueria')

    const outputWithoutAnNLine = 'p4345\nfcwd\n'
    expect(parseCwdOutput(outputWithoutAnNLine)).toBeNull()

    const outputWithAnEmptyNLine = 'p4345\nfcwd\nn\n'
    expect(parseCwdOutput(outputWithAnEmptyNLine)).toBeNull()
  })
})
