import { execFileSync, spawn, spawnSync } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

const SCRIPTS = dirname(fileURLToPath(import.meta.url))
const INSTALL_SCRIPT = join(SCRIPTS, '..', 'install.sh')
const APP_NAME = 'Taco Shells.app'

const tempDirectories = []

afterEach(() => {
  for (const directory of tempDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true })
  }
})

function makeTempDirectory() {
  const directory = mkdtempSync(join(tmpdir(), 'taco-shells-install-'))
  tempDirectories.push(directory)
  return directory
}

function writeMarker(bundle, text) {
  const contents = join(bundle, 'Contents')
  mkdirSync(contents, { recursive: true })
  writeFileSync(join(contents, 'marker'), text)
}

function readMarker(installDirectory) {
  const markerPath = join(installDirectory, APP_NAME, 'Contents', 'marker')
  return readFileSync(markerPath, 'utf8')
}

function makeFixtureZip(bundleName = APP_NAME) {
  const directory = makeTempDirectory()
  const bundle = join(directory, bundleName)
  const zip = join(directory, 'app.zip')
  writeMarker(bundle, 'new')
  execFileSync('ditto', ['-c', '-k', '--keepParent', bundle, zip])
  return zip
}

function makeOpenStub() {
  const directory = makeTempDirectory()
  const record = join(directory, 'open-record')
  const stub = join(directory, 'open')
  writeFileSync(stub, `#!/bin/sh\necho "$@" >> "${record}"\n`)
  chmodSync(stub, 0o755)
  return { directory, record }
}

function makeCommandFail(openStub, command) {
  const stub = join(openStub.directory, command)
  writeFileSync(stub, '#!/bin/sh\nexit 1\n')
  chmodSync(stub, 0o755)
}

function install(installDirectory, zipUrl, openStub) {
  const path = openStub.directory + delimiter + process.env.PATH
  const env = {
    ...process.env,
    TACO_SHELLS_INSTALL_DIR: installDirectory,
    TACO_SHELLS_ZIP_URL: zipUrl,
    PATH: path
  }
  execFileSync('sh', [INSTALL_SCRIPT], { env, stdio: 'pipe' })
}

function installFailureStatus(installDirectory, zipUrl, openStub) {
  try {
    install(installDirectory, zipUrl, openStub)
  } catch (error) {
    return error.status
  }
  return 0
}

function openWasCalled(openStub) {
  return existsSync(openStub.record)
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}

function isRunning(pattern) {
  try {
    execFileSync('pgrep', ['-f', pattern], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

async function waitUntilRunning(pattern) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (isRunning(pattern)) {
      return
    }
    await sleep(50)
  }
  throw new Error(`nothing matched ${pattern}`)
}

describe('install.sh', () => {
  it('installs the app and opens it', () => {
    const zip = makeFixtureZip()
    const installDirectory = makeTempDirectory()
    const openStub = makeOpenStub()

    install(installDirectory, `file://${zip}`, openStub)

    expect(readMarker(installDirectory)).toBe('new')
    const record = readFileSync(openStub.record, 'utf8')
    expect(record).toContain(join(installDirectory, APP_NAME))
  })

  it('replaces an install that is not running', () => {
    const zip = makeFixtureZip()
    const installDirectory = makeTempDirectory()
    const openStub = makeOpenStub()
    writeMarker(join(installDirectory, APP_NAME), 'old')

    install(installDirectory, `file://${zip}`, openStub)

    expect(readMarker(installDirectory)).toBe('new')
  })

  it('replaces an install it cannot delete', () => {
    const zip = makeFixtureZip()
    const installDirectory = makeTempDirectory()
    const openStub = makeOpenStub()
    const oldContents = join(installDirectory, APP_NAME, 'Contents')
    writeMarker(join(installDirectory, APP_NAME), 'old')
    chmodSync(oldContents, 0o555)

    try {
      install(installDirectory, `file://${zip}`, openStub)

      expect(readMarker(installDirectory)).toBe('new')
    } finally {
      execFileSync('chmod', ['-R', 'u+w', installDirectory])
    }
  })

  it('leaves the install alone when the download or the zip is bad', () => {
    const scratch = makeTempDirectory()
    const notAZip = join(scratch, 'not-a-zip.zip')
    writeFileSync(notAZip, 'this is not a zip')
    const zipOfAnotherApp = makeFixtureZip('Another.app')
    const badUrls = [
      `file://${join(scratch, 'missing.zip')}`,
      `file://${notAZip}`,
      `file://${zipOfAnotherApp}`
    ]

    for (const url of badUrls) {
      const installDirectory = makeTempDirectory()
      const openStub = makeOpenStub()
      writeMarker(join(installDirectory, APP_NAME), 'old')

      const status = installFailureStatus(installDirectory, url, openStub)

      expect(status).not.toBe(0)
      expect(readMarker(installDirectory)).toBe('old')
      expect(openWasCalled(openStub)).toBe(false)
    }
  })

  it('leaves the install alone when the new app cannot be moved in', () => {
    const zip = makeFixtureZip()
    const installDirectory = makeTempDirectory()
    const openStub = makeOpenStub()
    makeCommandFail(openStub, 'mv')
    writeMarker(join(installDirectory, APP_NAME), 'old')

    const status = installFailureStatus(installDirectory, `file://${zip}`, openStub)

    expect(status).not.toBe(0)
    expect(readMarker(installDirectory)).toBe('old')
  })

  it('refuses while the app is running', async () => {
    const zip = makeFixtureZip()
    const installDirectory = makeTempDirectory()
    const openStub = makeOpenStub()
    const bundle = join(installDirectory, APP_NAME)
    writeMarker(bundle, 'old')
    const macosDirectory = join(bundle, 'Contents', 'MacOS')
    mkdirSync(macosDirectory)
    const executable = join(macosDirectory, 'Taco Shells')
    writeFileSync(executable, '#!/bin/sh\nsleep 30\n')
    chmodSync(executable, 0o755)

    const child = spawn(executable, [], { stdio: 'ignore' })
    try {
      await waitUntilRunning(macosDirectory + '/')

      const status = installFailureStatus(installDirectory, `file://${zip}`, openStub)

      expect(status).not.toBe(0)
      expect(readMarker(installDirectory)).toBe('old')
    } finally {
      spawnSync('pkill', ['-P', String(child.pid)])
      child.kill()
    }
  })
})
