const STATUS_NOT_INSTALLED = 'Not installed'
const STATUS_OFF = 'Turned off in settings'
const STATUS_BY_HAND = 'Set up by hand'
const STATUS_SET_UP = 'Set up'
const STATUS_NOT_SET_UP = 'Not set up'
const LABEL_SWITCH = 'Switch to Managed Setup'
const LABEL_REMOVE = 'Remove Setup'

/** Who wired an agent's hooks in its settings file: nobody, the app, or the user by hand. */
export type HookWiring = 'none' | 'managed' | 'byHand'

export type AgentSetupState = 'notInstalled' | 'off' | 'byHand' | 'setUp' | 'declined' | 'notSetUp'

export interface AgentSetupFacts {
  installed: boolean
  /** The manageAgentHooks setting. */
  managing: boolean
  wiring: HookWiring
  declined: boolean
}

export type AgentMenuAction = 'setUp' | 'remove' | 'switch'

/** One agent's entry in the Agents menu: a status line and the one action its state allows. */
export interface AgentMenu {
  status: string
  action: AgentMenuAction
  actionLabel: string
  enabled: boolean
}

export function agentSetupStateOf(facts: AgentSetupFacts): AgentSetupState {
  if (!facts.installed) {
    return 'notInstalled'
  }
  if (!facts.managing) {
    return 'off'
  }
  if (facts.wiring === 'byHand') {
    return 'byHand'
  }
  if (facts.wiring === 'managed') {
    return 'setUp'
  }
  if (facts.declined) {
    return 'declined'
  }
  return 'notSetUp'
}

export function agentMenuOf(state: AgentSetupState, productName: string, canWrite: boolean): AgentMenu {
  const setUpLabel = `Set Up ${productName}`
  switch (state) {
    case 'notInstalled':
      return { status: STATUS_NOT_INSTALLED, action: 'setUp', actionLabel: setUpLabel, enabled: false }
    case 'off':
      return { status: STATUS_OFF, action: 'setUp', actionLabel: setUpLabel, enabled: false }
    case 'byHand':
      return { status: STATUS_BY_HAND, action: 'switch', actionLabel: LABEL_SWITCH, enabled: canWrite }
    case 'setUp':
      return { status: STATUS_SET_UP, action: 'remove', actionLabel: LABEL_REMOVE, enabled: canWrite }
    case 'declined':
    case 'notSetUp':
      return { status: STATUS_NOT_SET_UP, action: 'setUp', actionLabel: setUpLabel, enabled: canWrite }
  }
}
