import { existsSync, readFileSync } from 'node:fs'

import { appIsRunning, builtDmg } from './bundle.mjs'
import identity from './identity.cjs'
import { changeList, commandSucceeds, fail, git, lastTag, run, runWith, tryGit } from './repo.mjs'
import { isVersion, versionOfTag } from './version.mjs'

const STABLE_APP = identity.installedPathFor(identity.STABLE)

const INSTALL_NOTES = `## Installing

Download the dmg, open it, and drag Taco Shells into Applications. macOS refuses
to open it until the quarantine flag is cleared:

    xattr -dr com.apple.quarantine "/Applications/Taco Shells.app"

The app is ad-hoc signed and not notarised, so a downloaded copy is reported as
damaged rather than merely unverified until that attribute is gone.`

const args = process.argv.slice(2)
const publishing = args.includes('--publish')

function optionValue(name) {
  const prefix = `--${name}=`
  const given = args.find((argument) => argument.startsWith(prefix))
  if (!given) {
    return null
  }
  return given.slice(prefix.length)
}

// Promotion rewrites the release body, since the notes it carried as a
// prerelease say it has no download. A file replaces that generated body
// outright, for a release where the list of commits is not the story.
const notesFile = optionValue('notes-file')

// Promotion marks a version that already exists rather than minting one, so it
// runs where that version is: on the tag CI created for it.
const tag = tryGit('describe', '--tags', '--exact-match', '--match', 'v*', 'HEAD')
if (!tag) {
  fail(
    'HEAD is not exactly on a version tag. Promotion marks an existing version, so check out the tag you want to promote first.'
  )
}

const version = versionOfTag(tag)
if (!isVersion(version)) {
  fail(`${tag} is not a version tag.`)
}

// Checked here rather than where it is read, which is after the build.
if (notesFile && !existsSync(notesFile)) {
  fail(`no notes file at ${notesFile}.`)
}

const uncommitted = git('status', '--porcelain')
if (uncommitted) {
  fail(`the working tree has uncommitted changes, so a build would not be ${tag}:\n${uncommitted}`)
}

if (appIsRunning(STABLE_APP)) {
  fail(`${STABLE_APP} is running. Quit it so the promoted build can replace it.`)
}

if (publishing) {
  if (!commandSucceeds('gh', 'auth', 'status')) {
    fail('gh is not authenticated. Run `gh auth login`.')
  }
  if (!commandSucceeds('gh', 'release', 'view', tag)) {
    fail(`there is no GitHub release for ${tag} to promote.`)
  }
}

console.log(`Promoting ${version}. Running the test suites first.`)
run('npm', 'test')
run('npm', 'run', 'e2e')

// The only build that brands itself stable. Everything else, a plain
// `npm run install-app` included, produces the dev app instead.
const stableChannel = { [identity.CHANNEL_VARIABLE]: identity.STABLE_CHANNEL }
runWith(stableChannel, 'npm', 'run', 'dist')

const dmg = builtDmg()
if (!dmg) {
  fail('the build did not produce a dmg.')
}

// No channel needed: install-app finds whichever bundle was just built.
run('node', 'scripts/install-app.mjs')

if (!publishing) {
  let notesSource
  if (notesFile) {
    notesSource = notesFile
  } else {
    notesSource = 'the commits since the last release'
  }

  console.log(`
Dry run: ${tag} was not promoted on GitHub.

${STABLE_APP} is now ${version}, built and installed locally.

Adding --publish would attach ${dmg} to the ${tag} release, clear its prerelease
flag so GitHub lists it as Latest, and write its notes from ${notesSource}.
`)
  process.exit(0)
}

let notes
if (notesFile) {
  notes = readFileSync(notesFile, 'utf8')
} else {
  const previous = lastTag(`${tag}^`)
  notes = `${changeList(previous)}\n\n${INSTALL_NOTES}`
}

run('gh', 'release', 'upload', tag, dmg, '--clobber')
run('gh', 'release', 'edit', tag, '--prerelease=false', '--latest', '--notes', notes)

console.log(`
Promoted ${version}.
  release   ${tag} is now Latest, carrying ${dmg}
  installed ${STABLE_APP}
`)
