import { isVersion } from '@shared/version'

const TAG_PREFIX = 'v'

export interface ReleaseInfo {
  version: string
  assetUrl: string
  assetSize: number
}

interface ReleaseAsset {
  name: string
  size: number
  browser_download_url: string
}

/** The file name of the zip the updater downloads for a version. */
// Must match the artifactName template in electron-builder.js, or no release ever offers an update.
export function assetNameFor(version: string): string {
  return `taco-shells-${version}-arm64.zip`
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function findAsset(assets: unknown[], name: string): ReleaseAsset | null {
  for (const asset of assets) {
    if (isRecord(asset) && asset.name === name) {
      return asset as unknown as ReleaseAsset
    }
  }
  return null
}

/** The stable release described by a GitHub release payload, or null when it offers no usable update. */
export function parseRelease(payload: unknown): ReleaseInfo | null {
  if (!isRecord(payload)) {
    return null
  }
  if (payload.prerelease === true) {
    return null
  }
  const tag = payload.tag_name
  if (typeof tag !== 'string' || !tag.startsWith(TAG_PREFIX)) {
    return null
  }
  const version = tag.slice(TAG_PREFIX.length)
  if (!isVersion(version)) {
    return null
  }
  if (!Array.isArray(payload.assets)) {
    return null
  }
  const asset = findAsset(payload.assets, assetNameFor(version))
  if (asset === null) {
    return null
  }
  const assetUrl = asset.browser_download_url
  const assetSize = asset.size
  if (typeof assetUrl !== 'string') {
    return null
  }
  if (typeof assetSize !== 'number' || !Number.isFinite(assetSize) || assetSize <= 0) {
    return null
  }
  return { version, assetUrl, assetSize }
}
