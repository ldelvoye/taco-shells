import type { AgentSignal } from '@protocol/rows'

const NEEDS_TOOLS: ReadonlySet<string> = new Set(['AskUserQuestion', 'ExitPlanMode'])
const NEEDS_NOTIFICATIONS: ReadonlySet<string> = new Set([
  'permission_prompt',
  'elicitation_dialog',
  'elicitation_url_dialog',
  'agent_needs_input'
])

export function claudeSignalFor(event: string, detail: string | null): AgentSignal | null {
  if (event === 'UserPromptSubmit' || event === 'PostToolUse') {
    return 'working'
  }
  if (event === 'PreToolUse') {
    if (detail !== null && NEEDS_TOOLS.has(detail)) {
      return 'needs'
    }
    return 'working'
  }
  if (event === 'Notification') {
    if (detail !== null && NEEDS_NOTIFICATIONS.has(detail)) {
      return 'needs'
    }
    return null
  }
  if (event === 'Stop' || event === 'StopFailure') {
    return 'stopped'
  }
  return null
}
