const VERSION_PATTERN = /^\d+\.\d+\.\d+$/

/** Whether a string is a plain `X.Y.Z` release version. */
export function isVersion(value: string): boolean {
  return VERSION_PATTERN.test(value)
}

/** Whether `candidate` is a later release than `current`; false unless both are plain versions. */
export function isNewerVersion(candidate: string, current: string): boolean {
  if (!isVersion(candidate) || !isVersion(current)) {
    return false
  }
  const candidateParts = candidate.split('.').map(Number)
  const currentParts = current.split('.').map(Number)
  for (let index = 0; index < candidateParts.length; index++) {
    const candidatePart = candidateParts[index]
    const currentPart = currentParts[index]
    if (candidatePart !== currentPart) {
      return candidatePart > currentPart
    }
  }
  return false
}
