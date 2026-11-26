import type { ITerminalAddon, Terminal } from '@xterm/headless'

// The add-on's own typings import `@xterm/xterm`, which brings the DOM library into
// every file checked with it. tsconfig.base.json points the import here with no
// extension, so tsc finds this file and the bundler still loads the real package.
export class SerializeAddon implements ITerminalAddon {
  activate(terminal: Terminal): void
  dispose(): void
  serialize(): string
}
