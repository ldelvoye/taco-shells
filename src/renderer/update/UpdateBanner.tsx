import { type JSX, useState } from 'react'

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

  let action = (
    <button className="update-apply" onClick={onApply}>
      Restart to update
    </button>
  )
  if (problem !== null) {
    action = <p className="config-problem">{problem}</p>
  }

  return (
    <div className="config-banner update-banner">
      <p className="config-problem">Taco Shells {version} is ready.</p>
      {action}
      <button
        className="config-dismiss"
        aria-label="Dismiss"
        onClick={() => {
          setDismissed(version)
        }}
      >
        ×
      </button>
    </div>
  )
}
