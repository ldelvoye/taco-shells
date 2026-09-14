export const STABLE = 'stable'

const STABLE_NAME = 'Taco Shells'
const DEVELOPMENT_NAME = 'Taco Shells Dev'

// Anything not explicitly stable counts as development, so a build that lost its
// channel reads its own config rather than the one the released app depends on.

/** The name a build goes by, in the menu bar and in Applications. */
export function applicationName(channel: string): string {
  if (channel === STABLE) {
    return STABLE_NAME
  }
  return DEVELOPMENT_NAME
}

/** The directory a build reads its settings and keybindings from, under home. */
export function configDirectoryName(channel: string): string {
  if (channel === STABLE) {
    return '.taco-shells'
  }
  return '.taco-shells-dev'
}
