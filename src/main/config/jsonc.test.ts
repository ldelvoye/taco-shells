import { describe, expect, it } from 'vitest'
import { parseJsonc } from './jsonc'

describe('parseJsonc', () => {
  it('accepts comments and a trailing comma', () => {
    const text = '{\n  // the size in points\n  "fontSize": 14,\n}'
    const result = parseJsonc(text, 'settings.json')

    expect(result.value).toEqual({ fontSize: 14 })
    expect(result.problems).toEqual([])
  })

  it('refuses a partial object from a broken file, naming the line', () => {
    const text = '{\n  "fontSize": 14\n  "theme"\n}'
    const result = parseJsonc(text, 'settings.json')

    expect(result.value).toBeNull()
    expect(result.problems).toHaveLength(1)
    expect(result.problems[0]).toContain('settings.json')
    expect(result.problems[0]).toContain('line 3')
  })

  it('treats an empty file as no overrides', () => {
    const result = parseJsonc('', 'settings.json')

    expect(result.value).toEqual({})
    expect(result.problems).toEqual([])
  })

  it('reports a file too deeply nested to parse rather than throwing', () => {
    const depth = 50000
    const opening = '['.repeat(depth)
    const closing = ']'.repeat(depth)
    const text = `{ "fontSize": ${opening}1${closing} }`

    const result = parseJsonc(text, 'settings.json')

    expect(result.value).toBeNull()
    expect(result.problems).toEqual(['settings.json: the file is too deeply nested to read'])
  })

  it('refuses a top level that is not an object', () => {
    const result = parseJsonc('[1, 2]', 'keybindings.json')

    expect(result.value).toBeNull()
    expect(result.problems).toEqual(['keybindings.json: the file must hold a JSON object'])
  })
})
