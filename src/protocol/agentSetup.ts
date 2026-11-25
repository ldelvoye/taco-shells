export type AgentBannerKind = 'offer' | 'done' | 'problem'

/** One line in the strip about one agent: the offer to set it up, or how the last action went. */
export interface AgentBanner {
  harness: string
  kind: AgentBannerKind
  text: string
}
