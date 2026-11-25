import { useEffect, useState } from 'react'
import type { AgentBanner } from '@protocol/agentSetup'

/** The strips to show about agents: offers to set them up and how the last action went. */
export function useAgentBanners(): AgentBanner[] {
  const [banners, setBanners] = useState<AgentBanner[]>(window.tacoShells.agents.initial)

  useEffect(() => {
    return window.tacoShells.agents.onChange(setBanners)
  }, [])

  return banners
}
