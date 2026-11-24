import type { HarnessSetup } from '../types'

const ATTENTION_SCRIPT = 'taco-shells-attention.sh'
const SESSION_SCRIPT = 'taco-shells-claude-session.sh'

export const claudeSetup: HarnessSetup = {
  productName: 'Claude Code',
  configDirectoryVariable: 'CLAUDE_CONFIG_DIR',
  defaultConfigDirectory: '.claude',
  settingsFile: 'settings.json',
  entries: [
    { event: 'Notification', script: ATTENTION_SCRIPT, async: false },
    { event: 'Stop', script: ATTENTION_SCRIPT, async: false },
    { event: 'SessionStart', script: SESSION_SCRIPT, async: false },
    { event: 'SessionEnd', script: SESSION_SCRIPT, async: false },
    { event: 'UserPromptSubmit', script: SESSION_SCRIPT, async: true },
    { event: 'PreToolUse', script: SESSION_SCRIPT, async: true },
    { event: 'PostToolUse', script: SESSION_SCRIPT, async: true },
    { event: 'Notification', script: SESSION_SCRIPT, async: true },
    { event: 'Stop', script: SESSION_SCRIPT, async: true },
    { event: 'StopFailure', script: SESSION_SCRIPT, async: true }
  ]
}
