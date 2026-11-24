import type { JSX } from 'react'
import type { AgentBanner } from '@shared/agentSetup'

interface AgentBannersProps {
  banners: AgentBanner[]
  onSetUp: (harness: string) => void
  onDecline: (harness: string) => void
  onDismiss: (harness: string) => void
}

/** The strips offering to set up an agent, or saying how setting it up went. */
export function AgentBanners(props: AgentBannersProps): JSX.Element | null {
  const { banners, onSetUp, onDecline, onDismiss } = props
  if (banners.length === 0) {
    return null
  }

  const strips = banners.map((banner) => {
    let buttons = (
      <button
        className="config-dismiss"
        aria-label="Dismiss"
        onClick={() => {
          onDismiss(banner.harness)
        }}
      >
        ×
      </button>
    )
    if (banner.kind === 'offer') {
      buttons = (
        <>
          <button
            className="update-apply"
            onClick={() => {
              onSetUp(banner.harness)
            }}
          >
            Set up
          </button>
          <button
            className="update-apply agent-decline"
            onClick={() => {
              onDecline(banner.harness)
            }}
          >
            Not now
          </button>
        </>
      )
    }
    return (
      <div key={banner.harness} className="config-banner update-banner agent-banner">
        <p className="config-problem">{banner.text}</p>
        {buttons}
      </div>
    )
  })

  return <>{strips}</>
}
