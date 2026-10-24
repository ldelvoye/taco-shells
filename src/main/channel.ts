const STABLE = 'stable'

const STABLE_NAME = 'Taco Shells'
const DEVELOPMENT_NAME = 'Taco Shells Dev'
const STABLE_DIRECTORY = '.taco-shells'
const DEVELOPMENT_DIRECTORY = '.taco-shells-dev'

/**
 * Whether this build is the released Taco Shells rather than Taco Shells Dev.
 * Keep the comparison at module scope: only there does the bundler fold it and
 * drop the other channel's strings from the bundle.
 */
export const isStable = __CHANNEL__ === STABLE

// Anything not explicitly stable counts as development, so a build that lost its
// channel reads its own config rather than the one the released app depends on.
let buildName: string
let buildDirectory: string
if (isStable) {
  buildName = STABLE_NAME
  buildDirectory = STABLE_DIRECTORY
} else {
  buildName = DEVELOPMENT_NAME
  buildDirectory = DEVELOPMENT_DIRECTORY
}

/** The name this build goes by, in the menu bar and in Applications. */
export const applicationName = buildName

/** The directory this build reads its settings and keybindings from, under home. */
export const configDirectoryName = buildDirectory
