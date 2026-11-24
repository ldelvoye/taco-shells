import { type JSX, useState } from 'react'
import { Banner, BannerDismiss, BannerLine, BannerLines } from '../banner/Banner'

interface ConfigBannerProps {
  problems: string[]
}

export function ConfigBanner({ problems }: ConfigBannerProps): JSX.Element | null {
  const [dismissed, setDismissed] = useState<string | null>(null)
  const signature = problems.join('\n')

  if (problems.length === 0) {
    return null
  }
  if (dismissed === signature) {
    return null
  }

  return (
    <Banner kind="plain">
      <BannerLines>
        {problems.map((problem) => (
          <BannerLine key={problem}>{problem}</BannerLine>
        ))}
      </BannerLines>
      <BannerDismiss
        onDismiss={() => {
          setDismissed(signature)
        }}
      />
    </Banner>
  )
}
