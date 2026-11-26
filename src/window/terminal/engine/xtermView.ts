import { WebLinksAddon } from '@xterm/addon-web-links'
import { WebglAddon } from '@xterm/addon-webgl'
import { Terminal, type ILinkHandler, type ITerminalOptions } from '@xterm/xterm'
import '@xterm/xterm/css/xterm.css'
import type { PtySize } from '@protocol/ipc'
import type { Settings } from '@protocol/settings'
import type { TerminalColors } from '@protocol/theme'
import type { MakeTerminalView, TerminalView, TerminalViewOptions } from '../view'
import { clearInOrder, leaveAnswersToTheRuntime, loadSnapshot, resizeInOrder } from './stream'

const SCROLLBAR_WIDTH = 14
const MINIMUM_COLS = 2
const MINIMUM_ROWS = 1

// OSC 8 hyperlinks, where the text shown can differ from the URL behind it.
// Main decides which schemes may open, so xterm is told to pass them all.
function hyperlinkHandler(onLink: TerminalViewOptions['onLink']): ILinkHandler {
  return {
    activate: (event, url) => {
      onLink(event, url)
    },
    allowNonHttpProtocols: true
  }
}

function optionsFrom(
  settings: Settings,
  colors: TerminalColors,
  linkHandler: ILinkHandler
): ITerminalOptions {
  return {
    fontFamily: settings.fontFamily,
    fontSize: settings.fontSize,
    scrollback: settings.scrollback,
    scrollSensitivity: settings.scrollSensitivity,
    cursorBlink: settings.cursorBlink,
    cursorStyle: settings.cursorStyle,
    theme: colors,
    linkHandler
  }
}

function loadWebglRenderer(term: Terminal): void {
  try {
    const addon = new WebglAddon()
    addon.onContextLoss(() => {
      addon.dispose()
    })
    term.loadAddon(addon)
  } catch {
    // No WebGL context available; xterm keeps its DOM renderer.
  }
}

function pixels(style: CSSStyleDeclaration, property: string): number {
  const value = style.getPropertyValue(property)
  return parseInt(value)
}

// The fit add-on's arithmetic, for any box: a pane with no view of its own is sized from a view that has one.
function sizeThatFitsBox(term: Terminal, box: HTMLElement): PtySize | null {
  const element = term.element
  if (!element) {
    return null
  }
  const screen = element.querySelector('.xterm-screen')
  if (!screen) {
    return null
  }

  const screenBox = screen.getBoundingClientRect()
  const cellWidth = screenBox.width / term.cols
  const cellHeight = screenBox.height / term.rows
  if (!Number.isFinite(cellWidth) || !Number.isFinite(cellHeight)) {
    return null
  }
  if (cellWidth === 0 || cellHeight === 0) {
    return null
  }

  let scrollbarWidth = SCROLLBAR_WIDTH
  if (term.options.scrollback === 0) {
    scrollbarWidth = 0
  }

  const boxStyle = window.getComputedStyle(box)
  const boxHeight = pixels(boxStyle, 'height')
  const boxWidth = pixels(boxStyle, 'width')
  if (Number.isNaN(boxHeight) || Number.isNaN(boxWidth)) {
    return null
  }
  if (boxHeight === 0 || boxWidth === 0) {
    return null
  }

  const elementStyle = window.getComputedStyle(element)
  const paddingVertical = pixels(elementStyle, 'padding-top') + pixels(elementStyle, 'padding-bottom')
  const paddingHorizontal =
    pixels(elementStyle, 'padding-left') + pixels(elementStyle, 'padding-right')
  const availableHeight = boxHeight - paddingVertical
  const availableWidth = Math.max(0, boxWidth) - paddingHorizontal - scrollbarWidth

  const cols = Math.max(MINIMUM_COLS, Math.floor(availableWidth / cellWidth))
  const rows = Math.max(MINIMUM_ROWS, Math.floor(availableHeight / cellHeight))
  if (!Number.isFinite(cols) || !Number.isFinite(rows)) {
    return null
  }
  return { cols, rows }
}

export const makeXtermView: MakeTerminalView = (options) => {
  const linkHandler = hyperlinkHandler(options.onLink)
  const term = new Terminal(optionsFrom(options.settings, options.colors, linkHandler))

  const webLinks = new WebLinksAddon(options.onLink)
  term.loadAddon(webLinks)

  const element = document.createElement('div')
  element.className = 'terminal-host'

  leaveAnswersToTheRuntime(term)
  term.onData(options.onInput)

  let opened = false

  const view: TerminalView = {
    element,
    mount: () => {
      if (opened) {
        return
      }
      term.open(element)
      loadWebglRenderer(term)
      opened = true
    },
    load: (snapshot: string, size: PtySize) => {
      loadSnapshot(term, snapshot, size)
    },
    write: (data: string) => {
      term.write(data)
    },
    resize: (size: PtySize) => {
      resizeInOrder(term, size)
    },
    clear: () => {
      clearInOrder(term)
    },
    sizeThatFits: (box: HTMLElement) => sizeThatFitsBox(term, box),
    focus: () => {
      term.focus()
    },
    selection: () => term.getSelection(),
    paste: (text: string) => {
      term.paste(text)
    },
    type: (keys: string) => {
      term.input(keys)
    },
    apply: (settings: Settings, colors: TerminalColors) => {
      Object.assign(term.options, optionsFrom(settings, colors, linkHandler))
    },
    dispose: () => {
      term.dispose()
    }
  }
  return view
}
