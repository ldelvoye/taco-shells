import { execFile } from 'node:child_process'
import { createWriteStream, mkdirSync, readdirSync, renameSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { Readable } from 'node:stream'
import { pipeline } from 'node:stream/promises'
import { promisify } from 'node:util'
import type { ReleaseInfo } from './feed'

const execFileAsync = promisify(execFile)

const UPDATES_DIRECTORY_NAME = 'updates'
const ZIP_NAME = 'taco-shells.zip'
const STAGING_SUFFIX = '.partial'
const APP_SUFFIX = '.app'
const EXTRACT_TIMEOUT_MS = 120_000

/** The directory under the config directory that holds downloaded updates. */
export function updatesDirectory(configDirectory: string): string {
  return join(configDirectory, UPDATES_DIRECTORY_NAME)
}

function removeDirectory(directory: string): void {
  try {
    rmSync(directory, { recursive: true, force: true })
  } catch {
    return
  }
}

/** The entries of a directory, or none when it cannot be read. */
export function listChildren(directory: string): string[] {
  try {
    return readdirSync(directory)
  } catch {
    return []
  }
}

/** The single `.app` entry in a directory, or null when there are none or several. */
export function extractedAppIn(directory: string): string | null {
  const names = listChildren(directory)
  const apps = names.filter((name) => name.endsWith(APP_SUFFIX))
  if (apps.length !== 1) {
    return null
  }
  return join(directory, apps[0])
}

/** The removal of one version's directory from the updates directory. */
export function removeVersion(updatesDirectory: string, version: string): void {
  const directory = join(updatesDirectory, version)
  removeDirectory(directory)
}

/** The removal of every child of the updates directory except the one named `keep`. */
export function removeOtherVersions(updatesDirectory: string, keep: string | null): void {
  const names = listChildren(updatesDirectory)
  for (const name of names) {
    if (name === keep) {
      continue
    }
    const child = join(updatesDirectory, name)
    removeDirectory(child)
  }
}

async function writeCounted(response: Response, zipPath: string): Promise<number> {
  let bytes = 0
  const body = response.body
  if (body === null) {
    return -1
  }
  const source = Readable.fromWeb(body as Parameters<typeof Readable.fromWeb>[0])
  const counted = async function* (chunks: AsyncIterable<Buffer>): AsyncGenerator<Buffer> {
    for await (const chunk of chunks) {
      bytes += chunk.length
      yield chunk
    }
  }
  await pipeline(source, counted, createWriteStream(zipPath))
  return bytes
}

/** The path of the app extracted from a release's zip, or null when anything went wrong. */
export async function downloadAndExtract(
  release: ReleaseInfo,
  updatesDirectory: string,
  fetchImpl: typeof fetch
): Promise<string | null> {
  const versionDirectory = join(updatesDirectory, release.version)
  // Extracted under a name that is not a version, so a crash mid-extract leaves
  // nothing the next launch could adopt as a ready app.
  const stagingDirectory = `${versionDirectory}${STAGING_SUFFIX}`
  try {
    removeDirectory(stagingDirectory)
    mkdirSync(stagingDirectory, { recursive: true })
    const response = await fetchImpl(release.assetUrl, { cache: 'no-store' })
    if (!response.ok) {
      removeDirectory(stagingDirectory)
      return null
    }
    const zipPath = join(stagingDirectory, ZIP_NAME)
    const bytes = await writeCounted(response, zipPath)
    if (bytes !== release.assetSize) {
      removeDirectory(stagingDirectory)
      return null
    }
    await execFileAsync('ditto', ['-x', '-k', zipPath, stagingDirectory], { timeout: EXTRACT_TIMEOUT_MS })
    rmSync(zipPath, { force: true })
    const extracted = extractedAppIn(stagingDirectory)
    if (extracted === null) {
      removeDirectory(stagingDirectory)
      return null
    }
    removeDirectory(versionDirectory)
    renameSync(stagingDirectory, versionDirectory)
    return extractedAppIn(versionDirectory)
  } catch {
    removeDirectory(stagingDirectory)
    return null
  }
}
