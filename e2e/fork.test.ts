import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

import { KEY, launchApp } from './app'

const CLAUDE_SESSION_ID = '123e4567-e89b-12d3-a456-426614174000'

async function postToFocusServer(port: string, route: string, payload: object): Promise<void> {
  const body = JSON.stringify(payload)
  await fetch(`http://127.0.0.1:${port}${route}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body
  })
}

describe('fork with Claude', () => {
  it('forks a prompted Claude row into a colored, named split, until its group is full', async () => {
    const configDir = mkdtempSync(join(tmpdir(), 'taco-shells-fork-config-'))
    const scratch = mkdtempSync(join(tmpdir(), 'taco-shells-fork-scratch-'))
    const workDirPath = mkdtempSync(join(tmpdir(), 'taco-shells-fork-work-'))
    const workDir = realpathSync(workDirPath)
    const zshDir = mkdtempSync(join(tmpdir(), 'taco-shells-fork-zdotdir-'))
    const binDir = mkdtempSync(join(tmpdir(), 'taco-shells-fork-bin-'))

    const report = join(scratch, 'report')
    const pidFile = join(scratch, 'pid')
    const pwned = join(scratch, 'pwned')
    const title = `it's "q" $(touch ${pwned})`

    // A stand-in for claude: the function wins in zsh, where startup files put the
    // real one first on PATH, and the script covers every other login shell.
    const writeReport = `printf '%s\\n' "$PWD" "$@" > ${report}.part; mv ${report}.part ${report}`
    writeFileSync(join(zshDir, '.zshrc'), `claude() { ${writeReport}; }\n`)
    const scriptPath = join(binDir, 'claude')
    writeFileSync(scriptPath, `#!/bin/sh\n${writeReport}\n`)
    chmodSync(scriptPath, 0o755)

    const inheritedPath = process.env.PATH
    const env = { ZDOTDIR: zshDir, PATH: `${binDir}:${inheritedPath}` }
    const app = await launchApp({ configDir, env })
    try {
      await app.rightClickRow(0)
      await app.until('the picker is open', (state) => state.colorPickerOpen)
      const beforeSession = await app.screen()
      expect(beforeSession.forkItem).toBe('disabled')
      await app.press(KEY.escape)
      await app.until('the picker closes', (state) => !state.colorPickerOpen)

      const octalTitle = `\\033]0;✳ it\\047s \\042q\\042 \\044(touch ${pwned})\\007`
      const writePid = `printf '%s' $$ > ${pidFile}.part; mv ${pidFile}.part ${pidFile}`
      await app.runInPane('pty-1', `${writePid}; printf '${octalTitle}'; sleep 60`)
      await app.until('the shell reports its pid', () => existsSync(pidFile))
      const pidText = readFileSync(pidFile, 'utf8')
      const pid = Number(pidText)
      const portText = readFileSync(join(configDir, 'focus-port'), 'utf8')
      const port = portText.trim()

      const hook = { harness: 'claude', pids: [pid], sessionId: CLAUDE_SESSION_ID }
      await postToFocusServer(port, '/agent-session/start', { ...hook, cwd: workDir })
      await app.until('the row is known to run Claude', (state) => state.rows[0].label === title)

      await app.rightClickRow(0)
      const unprompted = await app.until('the picker is open', (state) => state.colorPickerOpen)
      expect(unprompted.forkItem).toBe('disabled')
      await app.clickColorDot('blue')
      await app.until('the picker closes', (state) => !state.colorPickerOpen)

      await postToFocusServer(port, '/agent-session/state', { ...hook, event: 'UserPromptSubmit' })
      await app.rightClickRow(0)
      await app.until('the fork item is offered', (state) => state.forkItem === 'enabled')
      await app.clickForkItem()

      const forked = await app.until('the fork opens', (state) => state.rows.length === 2)
      expect(forked.groups).toBe(1)
      expect(forked.rows.map((row) => row.color)).toEqual(['blue', 'blue'])
      expect(forked.rows.map((row) => row.active)).toEqual([false, true])
      expect(forked.colorPickerOpen).toBe(false)

      await app.until('claude receives the fork', () => existsSync(report))
      const reportText = readFileSync(report, 'utf8')
      const lines = reportText.trimEnd().split('\n')
      const forkName = `⑂ ${title}`
      const resume = ['--resume', CLAUDE_SESSION_ID, '--fork-session']
      expect(lines).toEqual([workDir, ...resume, '--name', forkName])
      expect(existsSync(pwned)).toBe(false)

      for (let panes = 3; panes <= 10; panes += 1) {
        await app.press(KEY.backslash, { cmd: true })
        await app.until(`the group holds ${panes} panes`, (state) => state.rows.length === panes)
      }
      await app.rightClickRow(0)
      const full = await app.until('the picker is open', (state) => state.colorPickerOpen)
      expect(full.forkItem).toBe('disabled')
    } finally {
      await app.close()
      for (const directory of [configDir, scratch, workDirPath, zshDir, binDir]) {
        rmSync(directory, { recursive: true, force: true })
      }
    }
  })
})
