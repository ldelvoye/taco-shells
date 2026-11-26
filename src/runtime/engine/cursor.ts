import type { Terminal } from '@xterm/headless'

const CURSOR_MODE = 25

function carriesCursorMode(params: (number | number[])[]): boolean {
  return params.includes(CURSOR_MODE)
}

// xterm keeps this to itself, so it is followed from the sequences that change it:
// the mode being set or reset, and the two resets that show the cursor again.
// Every handler answers false, which leaves xterm to act on the sequence as usual.
export function trackCursorHidden(term: Terminal): () => boolean {
  let hidden = false

  term.parser.registerCsiHandler({ prefix: '?', final: 'l' }, (params) => {
    if (carriesCursorMode(params)) {
      hidden = true
    }
    return false
  })
  term.parser.registerCsiHandler({ prefix: '?', final: 'h' }, (params) => {
    if (carriesCursorMode(params)) {
      hidden = false
    }
    return false
  })
  term.parser.registerCsiHandler({ intermediates: '!', final: 'p' }, () => {
    hidden = false
    return false
  })
  term.parser.registerEscHandler({ final: 'c' }, () => {
    hidden = false
    return false
  })

  return () => hidden
}
