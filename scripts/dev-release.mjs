import { changeList, changesSince, fail, lastTag, messagesSince, run, tryGit } from './repo.mjs'
import { FIRST_VERSION, nextVersion, tagFor, versionOfTag } from './version.mjs'

const NO_BINARY_NOTE = `This release is a point in main's history, not a build to install. It carries
no download: a dmg is large and you can build one in about forty seconds with
\`npm run install-app\`, which installs it as **Taco Shells Dev**, beside the
stable app rather than over it.

The current stable version is whichever release GitHub marks as Latest.`

const target = process.env.GITHUB_SHA
if (!target) {
  fail('GITHUB_SHA is not set; this is meant to run in CI.')
}

const previous = lastTag()
const changes = changesSince(previous)
if (changes.length === 0) {
  console.log(`nothing new since ${previous}. Nothing to release.`)
  process.exit(0)
}

let version
if (previous) {
  const messages = messagesSince(previous)
  version = nextVersion(versionOfTag(previous), messages)
} else {
  version = FIRST_VERSION
}
const tag = tagFor(version)

// Re-running a workflow must not fail on the tag it already made.
const existing = tryGit('rev-parse', '--verify', `refs/tags/${tag}`)
if (existing) {
  console.log(`${tag} already exists. Nothing to do.`)
  process.exit(0)
}

const notes = `${changeList(previous)}\n\n${NO_BINARY_NOTE}`

// --prerelease so it never becomes Latest. Promoting is what clears that, and
// until then the stable release stays the one people are pointed at.
run('gh', 'release', 'create', tag, '--prerelease', '--target', target, '--title', version, '--notes', notes)

console.log(`released ${tag} at ${target} (unstable)`)
