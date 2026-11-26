import { evaluate, type Protocol } from './connection'
import { pollFor } from './poll'

// xterm gives a lost graphics context three seconds to come back before it falls
// back to drawing rows as elements. Ten covers that with room to spare.
const FALLBACK_TIMEOUT_MS = 10000

// A terminal draws on a canvas, which holds no text to read. Losing its graphics
// context makes xterm draw its rows as elements, the way it does when a window has
// run out of contexts, and those can be read.
const READ_SHOWN_PANES = `JSON.stringify((() => {
  const shown = '.pane-group:not(.is-inactive) .terminal-host'
  for (const canvas of document.querySelectorAll(shown + ' canvas')) {
    const context = canvas.getContext('webgl2')
    if (context === null || context.isContextLost()) {
      continue
    }
    const extension = context.getExtension('WEBGL_lose_context')
    if (extension !== null) {
      extension.loseContext()
    }
  }

  const texts = []
  for (const host of document.querySelectorAll(shown)) {
    const rows = host.querySelector('.xterm-rows')
    if (rows === null) {
      return null
    }
    texts.push(rows.innerText)
  }
  return texts
})())`

/** The text of each terminal on screen, left to right. */
export async function shownText(page: Protocol): Promise<string[]> {
  return pollFor(
    'the terminals on screen to be readable',
    async () => {
      const read = await evaluate(page, READ_SHOWN_PANES)
      const texts: string[] | null = JSON.parse(read)
      return texts
    },
    FALLBACK_TIMEOUT_MS
  )
}

export async function untilShown(
  page: Protocol,
  description: string,
  holds: (texts: string[]) => boolean
): Promise<string[]> {
  return pollFor(
    description,
    async () => {
      const texts = await shownText(page)
      if (holds(texts)) {
        return texts
      }
      return null
    },
    FALLBACK_TIMEOUT_MS
  )
}
