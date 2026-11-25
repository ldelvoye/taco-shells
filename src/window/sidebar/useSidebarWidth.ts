import { useCallback, useEffect, useState } from 'react'
import {
  parseRemembered,
  rememberWidth,
  type RememberedWidths,
  widthForScreen,
  widthWithin
} from './width'

const STORAGE_KEY = 'taco-shells.sidebar-width'

export interface SidebarWidth {
  width: number
  resize: (width: number) => void
}

function readRemembered(): RememberedWidths {
  const stored = window.localStorage.getItem(STORAGE_KEY)
  return parseRemembered(stored)
}

function writeRemembered(remembered: RememberedWidths): void {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(remembered))
}

export function useSidebarWidth(): SidebarWidth {
  const [windowWidth, setWindowWidth] = useState(window.innerWidth)
  const [screenWidth, setScreenWidth] = useState(window.screen.width)
  const [chosenWidth, setChosenWidth] = useState(() => {
    const remembered = readRemembered()
    return widthForScreen(remembered, window.screen.width)
  })

  useEffect(() => {
    // Moving to another display is seen here only because the scale factor
    // changes with it. Two displays at the same scale factor fire nothing, so the
    // sidebar keeps the previous screen's width until the next resize.
    function handleResize(): void {
      setWindowWidth(window.innerWidth)

      const nextScreenWidth = window.screen.width
      if (nextScreenWidth === screenWidth) {
        return
      }
      const remembered = readRemembered()
      setScreenWidth(nextScreenWidth)
      setChosenWidth(widthForScreen(remembered, nextScreenWidth))
    }

    window.addEventListener('resize', handleResize)
    return () => {
      window.removeEventListener('resize', handleResize)
    }
  }, [screenWidth])

  const resize = useCallback((width: number) => {
    const currentScreenWidth = window.screen.width
    const remembered = readRemembered()
    const updated = rememberWidth(remembered, currentScreenWidth, width)
    writeRemembered(updated)
    setScreenWidth(currentScreenWidth)
    setChosenWidth(width)
  }, [])

  const width = widthWithin(chosenWidth, windowWidth)

  return { width, resize }
}
