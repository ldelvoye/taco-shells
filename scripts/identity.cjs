// Which app a build becomes. A version says nothing about this, since 0.2.2 is
// the same number promoted or not, so the channel is asked for and defaults to
// development: anything that forgets produces a dev build rather than replacing
// the app in daily use.
//
// CommonJS because electron-builder.js is loaded as CommonJS and must share this
// with the ESM scripts rather than restate it.
const CHANNEL_VARIABLE = 'TACO_SHELLS_CHANNEL'
const STABLE_CHANNEL = 'stable'

// The app ids differ as well as the names, because two bundles claiming one id
// leaves LaunchServices to pick between them.
const STABLE = {
  productName: 'Taco Shells',
  appId: 'com.ldelvoye.tacoshells',
  icon: 'build/icon.icns'
}

const DEVELOPMENT = {
  productName: 'Taco Shells Dev',
  appId: 'com.ldelvoye.tacoshells.dev',
  icon: 'build/icon-dev.icns'
}

function identityFor(channel) {
  if (channel === STABLE_CHANNEL) {
    return STABLE
  }
  return DEVELOPMENT
}

/** The identity this process is building under, from the environment. */
function currentIdentity() {
  return identityFor(process.env[CHANNEL_VARIABLE])
}

function installedPathFor(identity) {
  return `/Applications/${identity.productName}.app`
}

module.exports = {
  CHANNEL_VARIABLE,
  STABLE_CHANNEL,
  STABLE,
  currentIdentity,
  installedPathFor
}
