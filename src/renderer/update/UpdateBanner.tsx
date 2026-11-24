import { type JSX, useState } from 'react'
import { Banner, BannerAction, BannerDismiss, BannerLine } from '../banner/Banner'

interface UpdateBannerProps {
  version: string | null
  problem: string | null
  onApply: () => void
}

/** The strip offering a downloaded update, or the reason it could not be applied. */
export function UpdateBanner({ version, problem, onApply }: UpdateBannerProps): JSX.Element | null {
  const [dismissed, setDismissed] = useState<string | null>(null)

  if (version === null) {
    return null
  }
  if (dismissed === version) {
    return null
  }

  let action = <BannerAction onClick={onApply}>Restart to update</BannerAction>
  if (problem !== null) {
    action = <BannerLine>{problem}</BannerLine>
  }

  return (
    <Banner kind="notice">
      <BannerLine>Taco Shells {version} is ready.</BannerLine>
      {action}
      <BannerDismiss
        onDismiss={() => {
          setDismissed(version)
        }}
      />
    </Banner>
  )
}
