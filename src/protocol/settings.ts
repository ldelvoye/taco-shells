export type ThemeChoice = 'system' | 'light' | 'dark'
export type CursorStyle = 'block' | 'underline' | 'bar'

export interface Settings {
  theme: ThemeChoice
  fontFamily: string
  fontSize: number
  scrollback: number
  scrollSensitivity: number
  cursorBlink: boolean
  cursorStyle: CursorStyle
  restoreOnLaunch: boolean
  manageAgentHooks: boolean
}
