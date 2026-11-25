import type { AgentBanner } from '@protocol/agentSetup'

const STATUS_NOT_INSTALLED = 'Not installed'
const STATUS_OFF = 'Turned off in settings'
const STATUS_BY_HAND = 'Set up by hand'
const STATUS_SET_UP = 'Set up'
const STATUS_NOT_SET_UP = 'Not set up'
const LABEL_SWITCH = 'Switch to Managed Setup'
const LABEL_REMOVE = 'Remove Setup'
const NOTE_BUILD_MAY_NOT_WRITE = 'Development builds leave your settings alone'
// A CLI installed through npm shows in the process list as "node <path>/<name>".
const SCRIPT_RUNNER = 'node'

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
  /** Why the action is greyed out, when the status line does not already say. */
  note: string | null
}

export type AgentNotice = { kind: 'done' } | { kind: 'problem'; text: string }

function offerText(productName: string): string {
  return `Set up ${productName} to see which session needs you, restore conversations and fork them.`
}

function doneText(productName: string): string {
  return `${productName} is set up. Markers start with your next session.`
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

function noteOf(canWrite: boolean): string | null {
  if (canWrite) {
    return null
  }
  return NOTE_BUILD_MAY_NOT_WRITE
}

export function agentMenuOf(state: AgentSetupState, productName: string, canWrite: boolean): AgentMenu {
  const setUpLabel = `Set Up ${productName}`
  const note = noteOf(canWrite)
  switch (state) {
    case 'notInstalled':
      return {
        status: STATUS_NOT_INSTALLED,
        action: 'setUp',
        actionLabel: setUpLabel,
        enabled: false,
        note: null
      }
    case 'off':
      return { status: STATUS_OFF, action: 'setUp', actionLabel: setUpLabel, enabled: false, note: null }
    case 'byHand':
      return { status: STATUS_BY_HAND, action: 'switch', actionLabel: LABEL_SWITCH, enabled: canWrite, note }
    case 'setUp':
      return { status: STATUS_SET_UP, action: 'remove', actionLabel: LABEL_REMOVE, enabled: canWrite, note }
    case 'declined':
    case 'notSetUp':
      return { status: STATUS_NOT_SET_UP, action: 'setUp', actionLabel: setUpLabel, enabled: canWrite, note }
  }
}

/** What an action leaves in the strip: its problem, a done line after setting up, nothing after a removal. */
export function agentNoticeAfter(action: AgentMenuAction, problem: string | null): AgentNotice | null {
  if (problem !== null) {
    return { kind: 'problem', text: problem }
  }
  if (action === 'remove') {
    return null
  }
  return { kind: 'done' }
}

export function agentBannerOf(
  harness: string,
  productName: string,
  state: AgentSetupState,
  canWrite: boolean,
  notice: AgentNotice | null,
  seen: boolean
): AgentBanner | null {
  if (notice !== null && notice.kind === 'problem') {
    return { harness, kind: 'problem', text: notice.text }
  }
  // A done line outlives the click that earned it only while the agent is still set up.
  if (notice !== null && state === 'setUp') {
    const text = doneText(productName)
    return { harness, kind: 'done', text }
  }
  if (state === 'notSetUp' && canWrite && seen) {
    const text = offerText(productName)
    return { harness, kind: 'offer', text }
  }
  return null
}

function baseNameOf(word: string): string {
  const slash = word.lastIndexOf('/')
  return word.slice(slash + 1)
}

function runsCommand(commandLine: string, commands: readonly string[]): boolean {
  const pieces = commandLine.split(/\s+/)
  const words = pieces.filter((piece) => piece !== '')
  if (words.length === 0) {
    return false
  }
  const program = baseNameOf(words[0])
  if (commands.includes(program)) {
    return true
  }
  if (program !== SCRIPT_RUNNER || words.length < 2) {
    return false
  }
  const script = baseNameOf(words[1])
  return commands.includes(script)
}

/** Whether one of a terminal's foreground programs is the agent's CLI. */
export function runsAgent(commandLines: readonly string[], commands: readonly string[]): boolean {
  return commandLines.some((commandLine) => runsCommand(commandLine, commands))
}
