export const DEFAULT_SIDEBAR_WIDTH = 220
export const MIN_SIDEBAR_WIDTH = 140
export const MAX_SIDEBAR_SHARE = 0.5

export type RememberedWidths = Record<string, number>

function isCleanEntry(key: string, value: unknown): value is number {
  const keyAsNumber = Number(key)
  if (!Number.isFinite(keyAsNumber) || keyAsNumber <= 0) {
    return false
  }

  if (typeof value !== 'number') {
    return false
  }
  return Number.isFinite(value) && value > 0
}

export function parseRemembered(stored: string | null): RememberedWidths {
  if (stored === null) {
    return {}
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(stored)
  } catch {
    return {}
  }

  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return {}
  }

  const entries = Object.entries(parsed as Record<string, unknown>)
  const cleaned: RememberedWidths = {}
  for (const [key, value] of entries) {
    if (isCleanEntry(key, value)) {
      cleaned[key] = value
    }
  }
  return cleaned
}

export function widthForScreen(remembered: RememberedWidths, screenWidth: number): number {
  const exactKey = String(screenWidth)
  if (exactKey in remembered) {
    return remembered[exactKey]
  }

  const keys = Object.keys(remembered)
  if (keys.length === 0) {
    return DEFAULT_SIDEBAR_WIDTH
  }

  const firstKey = keys[0]
  let nearestKey = firstKey
  let nearestKeyWidth = Number(firstKey)
  let nearestDistance = Math.abs(nearestKeyWidth - screenWidth)

  for (let index = 1; index < keys.length; index += 1) {
    const key = keys[index]
    const keyWidth = Number(key)
    const distance = Math.abs(keyWidth - screenWidth)

    // A tie goes to the narrower screen, so the answer does not depend on the
    // order the keys happen to come back in.
    let closer = false
    if (distance < nearestDistance) {
      closer = true
    } else if (distance === nearestDistance && keyWidth < nearestKeyWidth) {
      closer = true
    }

    if (closer) {
      nearestKey = key
      nearestKeyWidth = keyWidth
      nearestDistance = distance
    }
  }

  return remembered[nearestKey]
}

export function widthWithin(width: number, windowWidth: number): number {
  const ceiling = windowWidth * MAX_SIDEBAR_SHARE

  let aboveFloor = width
  if (aboveFloor < MIN_SIDEBAR_WIDTH) {
    aboveFloor = MIN_SIDEBAR_WIDTH
  }

  let belowCeiling = aboveFloor
  if (belowCeiling > ceiling) {
    belowCeiling = ceiling
  }

  return belowCeiling
}

export function rememberWidth(
  remembered: RememberedWidths,
  screenWidth: number,
  width: number
): RememberedWidths {
  const key = String(screenWidth)
  const rounded = Math.round(width)
  return { ...remembered, [key]: rounded }
}
