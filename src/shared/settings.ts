export type ThemeChoice = 'system' | 'light' | 'dark'
export type CursorStyle = 'block' | 'underline' | 'bar'

export interface Settings {
  theme: ThemeChoice
  fontFamily: string
  fontSize: number
  scrollback: number
  scrollSensitivity: number
  cursorBlink: boolean
  cursorStyle: CursorStyle
}

export const DEFAULT_SETTINGS: Settings = {
  theme: 'system',
  fontFamily: "Menlo, Monaco, 'Courier New', monospace",
  fontSize: 12,
  scrollback: 100000,
  scrollSensitivity: 1,
  cursorBlink: true,
  cursorStyle: 'block'
}

export interface SettingsLayerResult {
  settings: Settings
  problems: string[]
}

interface SettingRule<Value> {
  check: (value: unknown) => value is Value
  requirement: string
}

// One rule per setting, each holding the type of the setting it guards, so a
// rule paired with the wrong key does not compile.
type SettingRules = { [Key in keyof Settings]: SettingRule<Settings[Key]> }

const FONT_SIZE_MIN = 1
const FONT_SIZE_MAX = 72

const THEME_CHOICES: ReadonlySet<string> = new Set(['system', 'light', 'dark'])
const CURSOR_STYLES: ReadonlySet<string> = new Set(['block', 'underline', 'bar'])

function isThemeChoice(value: unknown): value is ThemeChoice {
  return typeof value === 'string' && THEME_CHOICES.has(value)
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0
}

function isFontSize(value: unknown): value is number {
  const isNumber = typeof value === 'number' && Number.isFinite(value)
  return isNumber && value >= FONT_SIZE_MIN && value <= FONT_SIZE_MAX
}

function isScrollback(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0
}

function isPositiveNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
}

function isBoolean(value: unknown): value is boolean {
  return typeof value === 'boolean'
}

function isCursorStyle(value: unknown): value is CursorStyle {
  return typeof value === 'string' && CURSOR_STYLES.has(value)
}

const VALIDATORS: SettingRules = {
  theme: { check: isThemeChoice, requirement: 'must be "system", "light" or "dark"' },
  fontFamily: { check: isNonEmptyString, requirement: 'must be a non-empty string' },
  fontSize: { check: isFontSize, requirement: `must be a number between ${FONT_SIZE_MIN} and ${FONT_SIZE_MAX}` },
  scrollback: { check: isScrollback, requirement: 'must be a whole number of lines, 0 or more' },
  scrollSensitivity: { check: isPositiveNumber, requirement: 'must be a number greater than 0' },
  cursorBlink: { check: isBoolean, requirement: 'must be true or false' },
  cursorStyle: { check: isCursorStyle, requirement: 'must be "block", "underline" or "bar"' }
}

function isValidatedKey(key: string): key is keyof Settings {
  return Object.prototype.hasOwnProperty.call(VALIDATORS, key)
}

export function applySettingsLayer(base: Settings, layer: unknown, label: string): SettingsLayerResult {
  if (typeof layer !== 'object' || layer === null || Array.isArray(layer)) {
    return { settings: base, problems: [`${label}: the file must hold a JSON object`] }
  }

  const overrides: Record<string, unknown> = {}
  const problems: string[] = []

  const entries = Object.entries(layer)
  for (const [key, value] of entries) {
    if (!isValidatedKey(key)) {
      problems.push(`${label}: "${key}" is not a setting`)
      continue
    }

    const validator = VALIDATORS[key]
    if (!validator.check(value)) {
      problems.push(`${label}: "${key}" ${validator.requirement}`)
      continue
    }

    overrides[key] = value
  }

  const settings: Settings = { ...base, ...overrides }
  return { settings, problems }
}
