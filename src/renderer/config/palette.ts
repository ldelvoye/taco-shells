import type { ChromeColors, Palette } from '@shared/theme'

const CHROME_VARIABLES: Record<keyof ChromeColors, string> = {
  background: '--chrome-background',
  border: '--chrome-border',
  terminalBackground: '--terminal-background',
  text: '--text',
  textActive: '--text-active',
  rowHover: '--row-hover',
  rowActive: '--row-active',
  rowMarker: '--row-marker',
  rowConnector: '--row-connector',
  warningBackground: '--warning-background',
  warningBorder: '--warning-border',
  warningText: '--warning-text'
}

export function applyChromePalette(palette: Palette): void {
  const root = document.documentElement
  const entries = Object.entries(CHROME_VARIABLES) as Array<[keyof ChromeColors, string]>
  for (const [key, variable] of entries) {
    root.style.setProperty(variable, palette.chrome[key])
  }
}
