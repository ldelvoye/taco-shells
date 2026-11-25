import type { JSX } from 'react'
import markUrl from '../../../branding/icons/taco-shells-mark.svg'
import { PlusIcon } from './icons'
import { SidebarAction } from './SidebarAction'

const STABLE_CHANNEL = 'stable'
const WORDMARK = 'Taco Shells'

interface SidebarTitlebarProps {
  fullScreen: boolean
  onNewTerminal: () => void
}

export function SidebarTitlebar({ fullScreen, onNewTerminal }: SidebarTitlebarProps): JSX.Element {
  let className = 'sidebar-titlebar'
  if (fullScreen) {
    className += ' is-full-screen'
  }

  // The same turn build-icon.sh gives the dev app's Dock icon.
  let markClassName = 'sidebar-mark'
  if (__CHANNEL__ !== STABLE_CHANNEL) {
    markClassName += ' is-development'
  }

  return (
    <div className={className}>
      <div className="sidebar-brand">
        <img className={markClassName} src={markUrl} alt="" draggable={false} />
        <span className="sidebar-wordmark">{WORDMARK}</span>
      </div>
      <SidebarAction className="sidebar-new" label="New terminal" onActivate={onNewTerminal}>
        <PlusIcon />
      </SidebarAction>
    </div>
  )
}
