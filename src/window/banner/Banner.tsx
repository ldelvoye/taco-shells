import type { JSX, ReactNode } from 'react'

type BannerKind = 'plain' | 'notice' | 'agent'

interface BannerProps {
  kind: BannerKind
  children: ReactNode
}

interface BannerLinesProps {
  children: ReactNode
}

interface BannerLineProps {
  children: ReactNode
}

interface BannerDismissProps {
  onDismiss: () => void
}

interface BannerActionProps {
  quiet?: boolean
  onClick: () => void
  children: ReactNode
}

/** A strip above the terminals: plain for problems, notice for something to act on, agent for a notice about one agent. */
export function Banner({ kind, children }: BannerProps): JSX.Element {
  let className = 'banner'
  if (kind === 'notice') {
    className += ' is-notice'
  }
  if (kind === 'agent') {
    className += ' is-notice is-agent'
  }

  return <div className={className}>{children}</div>
}

export function BannerLines({ children }: BannerLinesProps): JSX.Element {
  return <div className="banner-lines">{children}</div>
}

export function BannerLine({ children }: BannerLineProps): JSX.Element {
  return <p className="banner-line">{children}</p>
}

export function BannerDismiss({ onDismiss }: BannerDismissProps): JSX.Element {
  return (
    <button className="banner-dismiss" aria-label="Dismiss" onClick={onDismiss}>
      ×
    </button>
  )
}

export function BannerAction({ quiet, onClick, children }: BannerActionProps): JSX.Element {
  let className = 'banner-action'
  if (quiet === true) {
    className += ' is-quiet'
  }

  return (
    <button className={className} onClick={onClick}>
      {children}
    </button>
  )
}
