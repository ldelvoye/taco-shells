import { execFileSync } from 'node:child_process'

import { appIsRunning, builtBundle, installedPathOf } from './bundle.mjs'

const bundle = builtBundle()
if (!bundle) {
  console.error('nothing to install: no single .app under dist.noindex. Run `npm run package` first.')
  process.exit(1)
}

const installed = installedPathOf(bundle)

// Replacing a bundle under a running process leaves the app misbehaving until it
// is relaunched. Quitting it here would take the user's open shells with it.
if (appIsRunning(installed)) {
  console.error(`${installed} is running. Quit it and run this again.`)
  process.exit(1)
}

execFileSync('ditto', [bundle, installed], { stdio: 'inherit' })
console.log(`installed ${installed}`)
