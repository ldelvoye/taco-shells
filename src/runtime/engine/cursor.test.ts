import { Terminal } from '@xterm/headless'
import { expect, it } from 'vitest'

import { trackCursorHidden } from './cursor'

async function hiddenAfter(output: string): Promise<boolean> {
  const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
  const hidden = trackCursorHidden(term)
  await new Promise<void>((resolve) => {
    term.write(output, resolve)
  })
  return hidden()
}

it('knows whether a program has hidden the cursor', async () => {
  const cases: [string, string, boolean][] = [
    ['nothing said', 'text', false],
    ['hidden', '\x1b[?25l', true],
    ['hidden among other modes', '\x1b[?1;25l', true],
    ['another mode turned off', '\x1b[?1l', false],
    ['hidden then shown', '\x1b[?25l\x1b[?25h', false],
    ['hidden then a full reset', '\x1b[?25l\x1bc', false],
    ['hidden then a soft reset', '\x1b[?25l\x1b[!p', false]
  ]

  for (const [name, output, expected] of cases) {
    expect(await hiddenAfter(output), name).toBe(expected)
  }
})
