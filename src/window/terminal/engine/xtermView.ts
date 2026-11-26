import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { WebglAddon } from '@xterm/addon-webgl'
import { Terminal, type ILinkHandler, type ITerminalOptions } from '@xterm/xterm'
import '@xterm/xterm/css/xterm.css'
import type { PtySize } from '@protocol/ipc'
import type { Settings } from '@protocol/settings'
import type { TerminalColors } from '@protocol/theme'
import type { MakeTerminalView, TerminalView, TerminalViewOptions } from '../view'
import { clearInOrder, leaveAnswersToTheRuntime, loadSnapshot, resizeInOrder } from './stream'

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

export const makeXtermView: MakeTerminalView = (options) => {
  const linkHandler = hyperlinkHandler(options.onLink)
  const term = new Terminal(optionsFrom(options.settings, options.colors, linkHandler))

  const fit = new FitAddon()
  term.loadAddon(fit)

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
    fittingSize: () => {
      const proposed = fit.proposeDimensions()
      if (!proposed) {
        return null
      }
      if (!Number.isFinite(proposed.cols) || !Number.isFinite(proposed.rows)) {
        return null
      }
      return { cols: proposed.cols, rows: proposed.rows }
    },
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
