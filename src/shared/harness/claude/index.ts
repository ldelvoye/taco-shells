import type { Harness } from '../types'
import { claudeForkCommand, claudeResumeCommand } from './commands'
import { isClaudeSessionId } from './sessionId'
import { claudeSetup } from './setup'
import { claudeSignalFor } from './signals'
import { claudeTitleOf } from './title'

export const claude: Harness = {
  id: 'claude',
  name: 'Claude',
  isSessionId: isClaudeSessionId,
  signalFor: claudeSignalFor,
  titleOf: claudeTitleOf,
  resumeCommand: claudeResumeCommand,
  forkCommand: claudeForkCommand,
  setup: claudeSetup
}
