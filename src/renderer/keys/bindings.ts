import defaultKeybindings from '@shared/keybindings.default.json'
import { type Keymap, resolveKeymap } from '@shared/keys'

const resolution = resolveKeymap(defaultKeybindings as Keymap)

if (resolution.problems.length > 0) {
  console.warn('keybindings:', resolution.problems.join('; '))
}

export const keyBindings = resolution.bindings
