import { expect, it } from 'vitest'

import { claudeSignalFor } from './signals'

it('maps hook events to signals, ignoring the idle reminder and subagents', () => {
  expect(claudeSignalFor('UserPromptSubmit', null)).toBe('working')
  expect(claudeSignalFor('PreToolUse', 'Bash')).toBe('working')
  expect(claudeSignalFor('PreToolUse', 'AskUserQuestion')).toBe('needs')
  expect(claudeSignalFor('Notification', 'permission_prompt')).toBe('needs')
  expect(claudeSignalFor('Notification', 'idle_prompt')).toBeNull()
  expect(claudeSignalFor('Notification', null)).toBeNull()
  expect(claudeSignalFor('Stop', null)).toBe('stopped')
  expect(claudeSignalFor('StopFailure', null)).toBe('stopped')
  expect(claudeSignalFor('SubagentStop', null)).toBeNull()
})
