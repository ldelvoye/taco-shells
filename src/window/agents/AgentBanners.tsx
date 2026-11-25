import type { JSX } from 'react'
import type { AgentBanner } from '@protocol/agentSetup'
import { Banner, BannerAction, BannerDismiss, BannerLine } from '../banner/Banner'

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
      <BannerDismiss
        onDismiss={() => {
          onDismiss(banner.harness)
        }}
      />
    )
    if (banner.kind === 'offer') {
      buttons = (
        <>
          <BannerAction
            onClick={() => {
              onSetUp(banner.harness)
            }}
          >
            Set up
          </BannerAction>
          <BannerAction
            quiet
            onClick={() => {
              onDecline(banner.harness)
            }}
          >
            Not now
          </BannerAction>
        </>
      )
    }
    return (
      <Banner key={banner.harness} kind="agent">
        <BannerLine>{banner.text}</BannerLine>
        {buttons}
      </Banner>
    )
  })

  return <>{strips}</>
}
