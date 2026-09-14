import { createRoot } from 'react-dom/client'
import { TITLEBAR_HEIGHT } from '@shared/chrome'
import { paletteFor } from '@shared/theme'
import '@xterm/xterm/css/xterm.css'
import './app.css'
import { App } from './App'
import { applyChromePalette } from './config/palette'

const container = document.getElementById('root')
if (!container) {
  throw new Error('renderer has no #root element to mount into')
}

// The same number positions the traffic lights over this strip in the main
// process, so it is set from there rather than written into the stylesheet too.
document.documentElement.style.setProperty('--titlebar-height', `${TITLEBAR_HEIGHT}px`)

const initialConfig = window.tacoShells.config.initial
const initialPalette = paletteFor(initialConfig.appearance)
applyChromePalette(initialPalette)

// No StrictMode: its deliberate double-invoking of effects would spawn a second
// shell for every terminal the app opens.
const root = createRoot(container)
root.render(<App />)
