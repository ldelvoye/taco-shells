import { posix } from 'node:path'

const SOURCE_FILE = /\.tsx?$/
const TEST_FILE = /\.test\.tsx?$/

const SPECIFIER_PATTERNS = [
  /\bfrom\s*(['"])([^'"\n]*)\1/g,
  /\bimport\s*(['"])([^'"\n]*)\1/g,
  /\bimport\s*\(\s*(['"])([^'"\n]*)\1/g,
  /\brequire\s*\(\s*(['"])([^'"\n]*)\1/g
]
const NON_LITERAL_IMPORT = /\bimport\s*\(\s*[^'"\s)]/g
// What importsOf reports for an import() whose argument is not a string, which no table can judge.
const NOT_A_LITERAL = ''

export interface Area {
  name: string
  folders: string[]
  mayImport: string[]
  modules: string[]
}

export interface Hole {
  file: string
  imports: string
  /** The number of the milestone that removes this import. */
  closes: number
}

export interface Violation {
  file: string
  line: number
  imports: string
  reason: string
}

export interface WallConfig {
  root: string
  aliases: Record<string, string>
  areas: Area[]
  testAreas: string[]
  testModules: string[]
  assetExtensions: string[]
}

interface Found {
  imports: string
  line: number
  index: number
}

function lineAt(source: string, index: number): number {
  const before = source.slice(0, index)
  const lines = before.split('\n')
  return lines.length
}

export function importsOf(source: string): { imports: string; line: number }[] {
  const found: Found[] = []

  for (const pattern of SPECIFIER_PATTERNS) {
    for (const match of source.matchAll(pattern)) {
      const specifier = match[2]
      const closingQuote = match.index + match[0].length - 1
      const index = closingQuote - specifier.length
      const line = lineAt(source, index)
      found.push({ imports: specifier, line, index })
    }
  }

  for (const match of source.matchAll(NON_LITERAL_IMPORT)) {
    const line = lineAt(source, match.index)
    found.push({ imports: NOT_A_LITERAL, line, index: match.index })
  }

  found.sort((a, b) => a.index - b.index)
  return found.map((entry) => ({ imports: entry.imports, line: entry.line }))
}

function areaOf(path: string, areas: Area[]): Area | undefined {
  // An import can name a folder for its index file, so the path is matched as a folder too.
  const asFolder = `${path}/`
  let best: Area | undefined
  let bestLength = 0
  for (const area of areas) {
    for (const folder of area.folders) {
      if (asFolder.startsWith(folder) && folder.length > bestLength) {
        best = area
        bestLength = folder.length
      }
    }
  }
  return best
}

function matchesPrefix(specifier: string, prefix: string): boolean {
  if (prefix.endsWith(':') || prefix.endsWith('/')) {
    return specifier.startsWith(prefix)
  }
  if (specifier === prefix) {
    return true
  }
  const folder = `${prefix}/`
  return specifier.startsWith(folder)
}

function aliasTarget(specifier: string, aliases: Record<string, string>): string | undefined {
  for (const [alias, folder] of Object.entries(aliases)) {
    if (specifier.startsWith(alias)) {
      const rest = specifier.slice(alias.length)
      return posix.normalize(`${folder}${rest}`)
    }
  }
  return undefined
}

function topFolder(path: string, root: string): string {
  const inside = path.slice(root.length)
  const segments = inside.split('/')
  return segments[0]
}

function resolveRelative(file: string, specifier: string): string {
  // Vite reads a query such as `?raw` off the end of a path.
  const parts = specifier.split('?')
  const bare = parts[0]
  const folder = posix.dirname(file)
  return posix.join(folder, bare)
}

function areaReason(area: Area, target: string, allowed: string[], areas: Area[]): string | undefined {
  const targetArea = areaOf(target, areas)
  if (targetArea === undefined) {
    return 'imports a file that belongs to no area'
  }
  if (allowed.includes(targetArea.name)) {
    return undefined
  }
  return `${area.name} may not import ${targetArea.name}`
}

function relativeReason(
  file: string,
  specifier: string,
  area: Area,
  allowed: string[],
  config: WallConfig
): string | undefined {
  const target = resolveRelative(file, specifier)

  if (!target.startsWith(config.root)) {
    const isAsset = config.assetExtensions.some((extension) => target.endsWith(extension))
    if (isAsset) {
      return undefined
    }
    return 'relative import leaves the source root and is not an asset'
  }

  const ownPackage = topFolder(file, config.root)
  const targetPackage = topFolder(target, config.root)
  if (ownPackage !== targetPackage) {
    return 'relative import leaves its package'
  }
  return areaReason(area, target, allowed, config.areas)
}

function moduleReason(specifier: string, area: Area, isTest: boolean, config: WallConfig): string | undefined {
  let prefixes = area.modules
  if (isTest) {
    prefixes = [...area.modules, ...config.testModules]
  }
  const allowed = prefixes.some((prefix) => matchesPrefix(specifier, prefix))
  if (allowed) {
    return undefined
  }
  return `${area.name} may not import the module ${specifier}`
}

function reasonFor(file: string, specifier: string, area: Area, config: WallConfig): string | undefined {
  if (specifier === NOT_A_LITERAL) {
    return 'dynamic import is not a string literal'
  }

  const isTest = TEST_FILE.test(file)
  const allowed = [area.name, ...area.mayImport]
  if (isTest) {
    allowed.push(...config.testAreas)
  }

  const target = aliasTarget(specifier, config.aliases)
  if (target !== undefined) {
    return areaReason(area, target, allowed, config.areas)
  }
  if (specifier.startsWith('.')) {
    return relativeReason(file, specifier, area, allowed, config)
  }
  return moduleReason(specifier, area, isTest, config)
}

export function violationsIn(files: ReadonlyMap<string, string>, config: WallConfig): Violation[] {
  const violations: Violation[] = []

  for (const [file, text] of files) {
    if (!file.startsWith(config.root) || !SOURCE_FILE.test(file)) {
      continue
    }

    const area = areaOf(file, config.areas)
    if (area === undefined) {
      violations.push({ file, line: 0, imports: '', reason: 'belongs to no area' })
      continue
    }

    for (const found of importsOf(text)) {
      const reason = reasonFor(file, found.imports, area, config)
      if (reason !== undefined) {
        violations.push({ file, line: found.line, imports: found.imports, reason })
      }
    }
  }

  return violations
}

function sameCrossing(violation: Violation, hole: Hole): boolean {
  return violation.file === hole.file && violation.imports === hole.imports
}

export function unlisted(violations: Violation[], holes: Hole[]): Violation[] {
  return violations.filter((violation) => !holes.some((hole) => sameCrossing(violation, hole)))
}

export function closed(violations: Violation[], holes: Hole[]): Hole[] {
  return holes.filter((hole) => !violations.some((violation) => sameCrossing(violation, hole)))
}
