import type { Terminal } from '@xterm/headless'

const CURSOR_MODE = 25

function carriesCursorMode(params: (number | number[])[]): boolean {
  return params.includes(CURSOR_MODE)
}

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
