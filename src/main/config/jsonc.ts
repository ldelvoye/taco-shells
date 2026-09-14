import { parse, printParseErrorCode, type ParseError } from 'jsonc-parser'

const PARSE_OPTIONS = { allowTrailingComma: true, allowEmptyContent: true }

const CAMEL_BOUNDARY = /([a-z])([A-Z])/g

export interface JsoncResult {
  value: Record<string, unknown> | null
  problems: string[]
}

function describeError(error: ParseError, text: string, label: string): string {
  const textBeforeError = text.slice(0, error.offset)
  const lineSegments = textBeforeError.split('\n')
  const newlineCount = lineSegments.length - 1
  const line = newlineCount + 1
  const code = printParseErrorCode(error.error)
  const spaced = code.replace(CAMEL_BOUNDARY, '$1 $2')
  const description = spaced.toLowerCase()
  return `${label}: ${description} at line ${line}`
}

export function parseJsonc(text: string, label: string): JsoncResult {
  const errors: ParseError[] = []

  // The parser recurses once per level of nesting and has no depth limit, so a
  // file nested a few thousand deep overflows the stack instead of reporting.
  let value: unknown
  try {
    value = parse(text, errors, PARSE_OPTIONS)
  } catch {
    return { value: null, problems: [`${label}: the file is too deeply nested to read`] }
  }

  // jsonc-parser returns whatever it managed to read alongside its errors, so a
  // half-parsed object arrives looking like a real one. Any error at all means the
  // file is not to be trusted.
  if (errors.length > 0) {
    const firstError = errors[0]
    const problem = describeError(firstError, text, label)
    return { value: null, problems: [problem] }
  }

  if (value === undefined) {
    return { value: {}, problems: [] }
  }

  const isObject = typeof value === 'object' && value !== null && !Array.isArray(value)
  if (!isObject) {
    return { value: null, problems: [`${label}: the file must hold a JSON object`] }
  }

  return { value: value as Record<string, unknown>, problems: [] }
}
