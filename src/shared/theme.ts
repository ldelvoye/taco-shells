export type Appearance = 'light' | 'dark'

export interface TerminalColors {
  background: string
  foreground: string
  cursor: string
  cursorAccent: string
  selectionBackground: string
  black: string
  red: string
  green: string
  yellow: string
  blue: string
  magenta: string
  cyan: string
  white: string
  brightBlack: string
  brightRed: string
  brightGreen: string
  brightYellow: string
  brightBlue: string
  brightMagenta: string
  brightCyan: string
  brightWhite: string
}

export interface ChromeColors {
  background: string
  border: string
  terminalBackground: string
  text: string
  textActive: string
  rowHover: string
  rowActive: string
  rowMarker: string
  rowConnector: string
  warningBackground: string
  warningBorder: string
  warningText: string
}

export interface Palette {
  terminal: TerminalColors
  chrome: ChromeColors
}

export const DARK_PALETTE: Palette = {
  terminal: {
    background: '#1e1e1e',
    foreground: '#cccccc',
    cursor: '#aeafad',
    cursorAccent: '#1e1e1e',
    selectionBackground: '#264f78',
    black: '#000000',
    red: '#cd3131',
    green: '#0dbc79',
    yellow: '#e5e510',
    blue: '#2472c8',
    magenta: '#bc3fbc',
    cyan: '#11a8cd',
    white: '#e5e5e5',
    brightBlack: '#666666',
    brightRed: '#f14c4c',
    brightGreen: '#23d18b',
    brightYellow: '#f5f543',
    brightBlue: '#3b8eea',
    brightMagenta: '#d670d6',
    brightCyan: '#29b8db',
    brightWhite: '#e5e5e5'
  },
  chrome: {
    background: '#252526',
    border: '#2d2d30',
    terminalBackground: '#1e1e1e',
    text: '#cccccc',
    textActive: '#ffffff',
    rowHover: '#2a2d2e',
    rowActive: '#37373d',
    rowMarker: '#0078d4',
    rowConnector: '#6e7681',
    warningBackground: '#352a05',
    warningBorder: '#b89500',
    warningText: '#cca700'
  }
}

export const LIGHT_PALETTE: Palette = {
  terminal: {
    background: '#ffffff',
    foreground: '#3b3b3b',
    cursor: '#3b3b3b',
    cursorAccent: '#ffffff',
    selectionBackground: '#add6ff',
    black: '#000000',
    red: '#cd3131',
    green: '#107c10',
    yellow: '#949800',
    blue: '#0451a5',
    magenta: '#bc05bc',
    cyan: '#0598bc',
    white: '#555555',
    brightBlack: '#666666',
    brightRed: '#cd3131',
    brightGreen: '#14ce14',
    brightYellow: '#b5ba00',
    brightBlue: '#0451a5',
    brightMagenta: '#bc05bc',
    brightCyan: '#0598bc',
    brightWhite: '#a5a5a5'
  },
  chrome: {
    background: '#f3f3f3',
    border: '#e5e5e5',
    terminalBackground: '#ffffff',
    text: '#3b3b3b',
    textActive: '#000000',
    rowHover: '#e8e8e8',
    rowActive: '#e4e6f1',
    rowMarker: '#005fb8',
    rowConnector: '#a0a0a0',
    warningBackground: '#f6f5d2',
    warningBorder: '#b89500',
    warningText: '#7a5b00'
  }
}

export function paletteFor(appearance: Appearance): Palette {
  if (appearance === 'dark') {
    return DARK_PALETTE
  } else {
    return LIGHT_PALETTE
  }
}
