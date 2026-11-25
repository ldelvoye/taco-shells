import { readFileSync, symlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { makeTempDirectory, removeTempDirectories } from '../testing/tempDirectory'
import { readConfigTexts, seedConfigFiles } from './files'

const PREFIX = 'taco-shells-config-'

afterEach(removeTempDirectories)

describe('seedConfigFiles', () => {
  it('writes both files exactly as shipped, comments and all', () => {
    const directory = makeTempDirectory(PREFIX)
    const settingsText = '{\n  // keep me\n  "fontSize": 12\n}'
    const problems = seedConfigFiles(directory, settingsText, '{}')

    expect(problems).toEqual([])
    expect(readFileSync(join(directory, 'settings.json'), 'utf8')).toBe(settingsText)
  })

  it('never overwrites a file the user has already written', () => {
    const directory = makeTempDirectory(PREFIX)
    writeFileSync(join(directory, 'settings.json'), '{ "fontSize": 20 }')
    seedConfigFiles(directory, '{ "fontSize": 12 }', '{}')

    expect(readFileSync(join(directory, 'settings.json'), 'utf8')).toBe('{ "fontSize": 20 }')
  })

  it('reports a file it left alone but cannot read', () => {
    const directory = makeTempDirectory(PREFIX)
    symlinkSync(join(directory, 'nowhere.json'), join(directory, 'settings.json'))

    const problems = seedConfigFiles(directory, '{ "fontSize": 12 }', '{}')

    expect(problems).toEqual([`${join(directory, 'settings.json')} exists but could not be read`])
  })

  it('reports a directory it cannot create instead of throwing', () => {
    const parent = makeTempDirectory(PREFIX)
    const blocker = join(parent, 'not-a-directory')
    writeFileSync(blocker, '')
    const problems = seedConfigFiles(join(blocker, 'taco-shells'), '{}', '{}')

    expect(problems).toHaveLength(1)
    expect(problems[0]).toContain('taco-shells')
  })
})

describe('readConfigTexts', () => {
  it('reads a file that is there and reports null for one that is not', () => {
    const directory = makeTempDirectory(PREFIX)
    writeFileSync(join(directory, 'settings.json'), '{ "fontSize": 20 }')
    const texts = readConfigTexts(directory)

    expect(texts.settings).toBe('{ "fontSize": 20 }')
    expect(texts.keybindings).toBeNull()
  })
})
