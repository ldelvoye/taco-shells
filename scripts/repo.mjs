import { execFileSync } from 'node:child_process'

// Node captures 1MB by default and throws a bare ENOBUFS past it. A log of every
// commit since the last tag clears that at around twenty thousand commits, which
// is a cryptic way to fail a release.
const MAX_OUTPUT_BYTES = 64 * 1024 * 1024

/** A command run with its output going straight to the terminal; throws if it fails. */
export function run(command, ...args) {
  execFileSync(command, args, { stdio: 'inherit' })
}

/** Like `run`, with extra environment variables for the child. */
export function runWith(environment, command, ...args) {
  execFileSync(command, args, {
    stdio: 'inherit',
    env: { ...process.env, ...environment }
  })
}

/** Whether a command exits zero, with its output discarded. */
export function commandSucceeds(command, ...args) {
  try {
    execFileSync(command, args, { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

export function fail(message) {
  console.error(message)
  process.exit(1)
}

/** A git command's trimmed output; throws if git fails. */
export function git(...args) {
  const output = execFileSync('git', args, {
    encoding: 'utf8',
    maxBuffer: MAX_OUTPUT_BYTES
  })
  return output.trim()
}

// Swallows git's stderr as well as the throw, so a legitimate "there isn't one"
// does not read as a failure in a CI log.
/** Like `git`, but null instead of throwing. */
export function tryGit(...args) {
  try {
    const output = execFileSync('git', args, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
      maxBuffer: MAX_OUTPUT_BYTES
    })
    return output.trim()
  } catch {
    return null
  }
}

/** The most recent version tag reachable from a commit, or null before the first one. */
export function lastTag(commit = 'HEAD') {
  return tryGit('describe', '--tags', '--abbrev=0', '--match', 'v*', commit)
}

function rangeSince(tag) {
  if (tag) {
    return `${tag}..HEAD`
  }
  return 'HEAD'
}

/** The commit subjects on HEAD since a tag, one per commit, for release notes. */
export function changesSince(tag) {
  const log = git('log', '--format=%s', rangeSince(tag))
  const lines = log.split('\n')
  return lines.filter((subject) => subject.length > 0)
}

// Subjects are not enough to choose a version: a conventional commit can declare
// a break in a footer, which lives in the body. Separated by NUL because a commit
// message may contain any other byte, blank lines included.
export function messagesSince(tag) {
  const log = git('log', '--format=%B%x00', rangeSince(tag))
  const messages = log.split('\0')
  const trimmed = messages.map((message) => message.trim())
  return trimmed.filter((message) => message.length > 0)
}

/** A markdown "Changes" section listing what landed since a tag. */
export function changeList(tag) {
  const changes = changesSince(tag)
  if (changes.length === 0) {
    return '## Changes\n\nNothing since the last release.'
  }

  const bullets = changes.map((subject) => `- ${subject}`)
  return `## Changes\n\n${bullets.join('\n')}`
}
