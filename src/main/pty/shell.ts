import { userInfo } from 'node:os'

const FALLBACK_SHELL = '/bin/zsh'

export function shellPath(): string {
  const fromUserRecord = userInfo().shell
  if (fromUserRecord) {
    return fromUserRecord
  }

  const fromEnvironment = process.env.SHELL
  if (fromEnvironment) {
    return fromEnvironment
  }

  return FALLBACK_SHELL
}

// A login shell, the way macOS terminals start one. What actually repairs
// launchd's Homebrew-less PATH is .zshrc, which runs only because node-pty gives
// the shell a tty and so makes it interactive.
export function shellArguments(): string[] {
  return ['-l']
}

export function shellEnvironment(): Record<string, string> {
  const environment: Record<string, string> = {}
  for (const [name, value] of Object.entries(process.env)) {
    if (value !== undefined) {
      environment[name] = value
    }
  }

  // Electron reads this variable to start as a bare node process. Inherited by
  // the shell, any Electron app launched from a terminal would come up as a node
  // repl instead of its own window.
  delete environment.ELECTRON_RUN_AS_NODE

  environment.TERM = 'xterm-256color'
  environment.COLORTERM = 'truecolor'
  if (!environment.LANG) {
    environment.LANG = 'en_US.UTF-8'
  }

  return environment
}
