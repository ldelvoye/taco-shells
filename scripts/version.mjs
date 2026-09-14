const VERSION = /^(\d+)\.(\d+)\.(\d+)$/

// Conventional commit subjects: `type(scope): summary`, with a `!` before the
// colon marking a breaking change.
const BREAKING_SUBJECT = /^[a-z]+(\([^)]*\))?!:/
const FEATURE_SUBJECT = /^feat(\([^)]*\))?:/
const BREAKING_BODY = 'BREAKING CHANGE'

/** The version a repository with no tags yet starts from. */
export const FIRST_VERSION = '0.1.0'

/** Whether a string is a plain three-number version. */
export function isVersion(version) {
  return VERSION.test(version)
}

function partsOf(version) {
  const matched = VERSION.exec(version)
  if (!matched) {
    throw new Error(`not a version: ${version}`)
  }

  return {
    major: Number(matched[1]),
    minor: Number(matched[2]),
    patch: Number(matched[3])
  }
}

/** The bump a set of full commit messages calls for: major, minor or patch. */
export function bumpFor(messages) {
  const breaking = messages.some((message) => {
    return BREAKING_SUBJECT.test(message) || message.includes(BREAKING_BODY)
  })
  if (breaking) {
    return 'major'
  }

  const feature = messages.some((message) => FEATURE_SUBJECT.test(message))
  if (feature) {
    return 'minor'
  }

  return 'patch'
}

/** The version after applying a bump. Below 1.0.0 a breaking change moves the minor. */
export function applyBump(version, bump) {
  const { major, minor, patch } = partsOf(version)

  if (bump === 'major') {
    if (major === 0) {
      return `0.${minor + 1}.0`
    }
    return `${major + 1}.0.0`
  }

  if (bump === 'minor') {
    return `${major}.${minor + 1}.0`
  }

  return `${major}.${minor}.${patch + 1}`
}

/** The version a set of commits lands on, given the one before them. */
export function nextVersion(current, messages) {
  const bump = bumpFor(messages)
  return applyBump(current, bump)
}

/** The git tag for a version, and the version for a tag. */
export function tagFor(version) {
  return `v${version}`
}

export function versionOfTag(tag) {
  return tag.replace(/^v/, '')
}
