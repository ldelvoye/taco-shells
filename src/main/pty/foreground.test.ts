import { describe, expect, it } from 'vitest'
import { parseForegroundCommands } from './foreground'

const PS_SAMPLE = [
  '    1     1     0 /sbin/launchd',
  '  501   501   777 -zsh',
  '  610   777   777 /Users/a/.local/bin/claude --resume abc',
  '  611   777   777 node /opt/tools/my server.js',
  '  700   700   888 -zsh',
  '  701   888   888 vim notes.txt',
  'this line is not a process row'
].join('\n')

describe('foreground programs', () => {
  it("lists the commands in the foreground of the shell's terminal", () => {
    expect(parseForegroundCommands(PS_SAMPLE, 501)).toEqual([
      '/Users/a/.local/bin/claude --resume abc',
      'node /opt/tools/my server.js'
    ])
    expect(parseForegroundCommands(PS_SAMPLE, 999)).toEqual([])
  })
})
