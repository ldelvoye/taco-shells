import { type JSX, useState } from 'react'

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
    <div className="config-banner">
      <div className="config-problems">
        {problems.map((problem) => (
          <p key={problem} className="config-problem">
            {problem}
          </p>
        ))}
      </div>
      <button
        className="config-dismiss"
        aria-label="Dismiss"
        onClick={() => {
          setDismissed(signature)
        }}
      >
        ×
      </button>
    </div>
  )
}
