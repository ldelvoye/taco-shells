import { describe, expect, it } from 'vitest'
import { applySettingsLayer, DEFAULT_SETTINGS } from './settings'

describe('applySettingsLayer', () => {
  it('takes the keys the layer sets and leaves the rest at the base', () => {
    const layer = { fontSize: 16, theme: 'light' }
    const result = applySettingsLayer(DEFAULT_SETTINGS, layer, 'settings.json')

    expect(result.settings.fontSize).toBe(16)
    expect(result.settings.theme).toBe('light')
    expect(result.settings.fontFamily).toBe(DEFAULT_SETTINGS.fontFamily)
    expect(result.problems).toEqual([])
  })

  it('drops only the bad key, keeping the good ones in the same layer', () => {
    const layer = { fontSize: 'large', scrollback: 500 }
    const result = applySettingsLayer(DEFAULT_SETTINGS, layer, 'settings.json')

    expect(result.settings.fontSize).toBe(DEFAULT_SETTINGS.fontSize)
    expect(result.settings.scrollback).toBe(500)
    expect(result.problems).toEqual(['settings.json: "fontSize" must be a number between 1 and 72'])
  })

  it('rejects values outside each key\'s range or set', () => {
    const layer = {
      fontSize: 0,
      scrollback: -1,
      scrollSensitivity: 0,
      fontFamily: '',
      theme: 'solarized',
      cursorStyle: 'beam',
      cursorBlink: 'yes'
    }
    const result = applySettingsLayer(DEFAULT_SETTINGS, layer, 'settings.json')

    expect(result.settings).toEqual(DEFAULT_SETTINGS)
    expect(result.problems).toHaveLength(7)
  })

  it('reports a key it does not know and ignores it', () => {
    const layer = { fontLigatures: true }
    const result = applySettingsLayer(DEFAULT_SETTINGS, layer, 'settings.json')

    expect(result.settings).toEqual(DEFAULT_SETTINGS)
    expect(result.problems).toEqual(['settings.json: "fontLigatures" is not a setting'])
  })

  it('reports a layer that is not an object and keeps the base', () => {
    const result = applySettingsLayer(DEFAULT_SETTINGS, [1, 2], 'settings.json')

    expect(result.settings).toEqual(DEFAULT_SETTINGS)
    expect(result.problems).toEqual(['settings.json: the file must hold a JSON object'])
  })
})
