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
  stateWorking: string
  stateNeeds: string
  stateDone: string
  rowColorRed: string
  rowColorOrange: string
  rowColorYellow: string
  rowColorGreen: string
  rowColorBlue: string
  rowColorPurple: string
  warningBackground: string
  warningBorder: string
  warningText: string
}

export interface Palette {
  terminal: TerminalColors
  chrome: ChromeColors
}
