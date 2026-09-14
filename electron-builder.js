const { execFileSync } = require('node:child_process')

const { version: packageVersion } = require('./package.json')
const { currentIdentity } = require('./scripts/identity.cjs')

// Tags are the version, not package.json: every push to main is tagged, and
// package.json's field is a stale floor nothing reads. `--match v*` keeps this
// clear of any other tag namespace.
function describeGit() {
  try {
    const described = execFileSync(
      'git',
      ['describe', '--tags', '--always', '--dirty', '--match', 'v*'],
      { encoding: 'utf8' }
    )
    return described.trim()
  } catch {
    return null
  }
}

// The version the About panel shows: the tag when HEAD is on one, git's
// `tag-count-gsha` notation past it, or the package floor before the first tag.
function displayVersion(described) {
  if (!described) {
    return packageVersion
  }

  if (described.startsWith('v')) {
    return described.slice(1)
  }

  return `${packageVersion}+${described}`
}

const described = describeGit()
const version = displayVersion(described)
const identity = currentIdentity()

let buildVersion
if (described) {
  buildVersion = described
} else {
  buildVersion = 'unknown'
}

module.exports = {
  appId: identity.appId,
  productName: identity.productName,

  // The bundle is "Taco Shells.app", but an asset named from productName reaches
  // a download link as Taco%20Shells. Name it from the package and the version
  // git reports, since package.json's own version is not authoritative.
  artifactName: 'taco-shells-' + version + '-${arch}.${ext}',

  // Spotlight skips a directory named .noindex, so the build output does not show
  // up as a second, staler Taco Shells beside the installed one.
  directories: {
    buildResources: 'build',
    output: 'dist.noindex'
  },

  files: [
    '!src',
    '!docs',
    '!branding',
    '!scripts',
    '!e2e',
    '!electron.vite.config.ts',
    '!vitest.config.ts',
    '!vitest.e2e.config.ts',
    '!tsconfig*.json',
    '!electron-builder.js'
  ],

  // node-pty ships a native binding and a spawn-helper executable, neither of
  // which macOS can load from inside an asar archive.
  asarUnpack: ['**/node_modules/node-pty/**'],

  mac: {
    category: 'public.app-category.developer-tools',
    icon: identity.icon,
    darkModeSupport: true,
    bundleShortVersion: version,
    bundleVersion: buildVersion,
    target: [
      {
        target: 'dmg',
        arch: 'arm64'
      }
    ]
  }
}
