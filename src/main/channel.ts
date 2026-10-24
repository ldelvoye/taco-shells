const STABLE = 'stable'

const STABLE_NAME = 'Taco Shells'
const DEVELOPMENT_NAME = 'Taco Shells Dev'
const STABLE_DIRECTORY = '.taco-shells'
const DEVELOPMENT_DIRECTORY = '.taco-shells-dev'

// Anything not explicitly stable counts as development, so a build that lost its
// channel reads its own config rather than the one the released app depends on.
// The comparison has to stay at module scope: the bundler folds it there and
// drops the other channel's strings, which it cannot do through a function
// argument.
/** Whether this build is the released Taco Shells rather than Taco Shells Dev. */
export const isStable = __CHANNEL__ === STABLE

let name: string
let directoryName: string
if (isStable) {
  name = STABLE_NAME
  directoryName = STABLE_DIRECTORY
} else {
  name = DEVELOPMENT_NAME
  directoryName = DEVELOPMENT_DIRECTORY
}

/** The name this build goes by, in the menu bar and in Applications. */
export const applicationName = name

/** The directory this build reads its settings and keybindings from, under home. */
export const configDirectoryName = directoryName
