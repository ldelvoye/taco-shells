const FIRST_DYNAMIC = 10
const LAST_DYNAMIC = 12
const LAST_INDEX = 255
const WHOLE_NUMBER = /^\d+$/
// The two ways xterm reads a color. Anything else it ignores, and what is kept here
// goes into every snapshot, so nothing else is kept.
const COLOR = /^(?:#(?:[0-9a-f]{3}){1,4}|rgb:[0-9a-f]{1,4}\/[0-9a-f]{1,4}\/[0-9a-f]{1,4})$/i

export type ColorSetSequence = 4 | 10 | 11 | 12
export type ColorResetSequence = 104 | 110 | 111 | 112

/** The colors a program has set for its terminal, kept so a view built later can be given them. */
export interface ColorSets {
  set(sequence: ColorSetSequence, data: string): void
  reset(sequence: ColorResetSequence, data: string): void
  /** The sequences that set them again, for the end of a snapshot. */
  sequences(): string
}

function sequenceFor(ident: string, color: string): string {
  return `\x1b]${ident};${color}\x1b\\`
}

export function trackColorSets(): ColorSets {
  const palette = new Map<number, string>()
  const dynamic = new Map<number, string>()

  function setPalette(data: string): void {
    const parts = data.split(';')
    for (let position = 0; position + 1 < parts.length; position += 2) {
      const index = parts[position]
      const color = parts[position + 1]
      if (!COLOR.test(color) || !WHOLE_NUMBER.test(index)) {
        continue
      }
      const number = parseInt(index, 10)
      if (number <= LAST_INDEX) {
        palette.set(number, color)
      }
    }
  }

  function setDynamic(first: number, data: string): void {
    const slots = data.split(';')
    for (let position = 0; position < slots.length; position += 1) {
      const ident = first + position
      if (ident > LAST_DYNAMIC) {
        break
      }
      const color = slots[position]
      if (COLOR.test(color)) {
        dynamic.set(ident, color)
      }
    }
  }

  function resetPalette(data: string): void {
    if (data === '') {
      palette.clear()
      return
    }
    const indexes = data.split(';')
    for (const index of indexes) {
      if (WHOLE_NUMBER.test(index)) {
        const number = parseInt(index, 10)
        palette.delete(number)
      }
    }
  }

  return {
    set(sequence: ColorSetSequence, data: string): void {
      if (sequence === 4) {
        setPalette(data)
        return
      }
      setDynamic(sequence, data)
    },
    reset(sequence: ColorResetSequence, data: string): void {
      if (sequence === 104) {
        resetPalette(data)
        return
      }
      const ident = sequence - 100
      dynamic.delete(ident)
    },
    sequences(): string {
      const all: string[] = []
      for (const [index, color] of palette) {
        all.push(sequenceFor(`4;${index}`, color))
      }
      for (let ident = FIRST_DYNAMIC; ident <= LAST_DYNAMIC; ident += 1) {
        const color = dynamic.get(ident)
        if (color !== undefined) {
          all.push(sequenceFor(String(ident), color))
        }
      }
      return all.join('')
    }
  }
}
