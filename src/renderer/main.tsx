import { createRoot } from 'react-dom/client'
import { TITLEBAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS_WIDTH } from '@shared/chrome'
import { paletteFor } from '@shared/theme'
import '@xterm/xterm/css/xterm.css'
import './app.css'
import { App } from './App'
import { applyChromePalette } from './config/palette'

const container = document.getElementById('root')
if (!container) {
  throw new Error('renderer has no #root element to mount into')
}

// The main process places the traffic lights over this strip from the same
// shared numbers, so they are set from there rather than written into the
// stylesheet too.
document.documentElement.style.setProperty('--titlebar-height', `${TITLEBAR_HEIGHT}px`)
document.documentElement.style.setProperty('--traffic-light-inset', `${TRAFFIC_LIGHT_INSET}px`)
document.documentElement.style.setProperty('--traffic-lights-width', `${TRAFFIC_LIGHTS_WIDTH}px`)

const initialConfig = window.tacoShells.config.initial
const initialPalette = paletteFor(initialConfig.appearance)
applyChromePalette(initialPalette)

// No StrictMode: its deliberate double-invoking of effects would spawn a second
// shell for every terminal the app opens.
const root = createRoot(container)
root.render(<App />)
