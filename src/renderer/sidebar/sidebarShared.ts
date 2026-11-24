import type { DropMark } from '@shared/dropMark'

export const RIGHT_BUTTON = 2

export function edgeClass(mark: DropMark): string {
  if (mark.edge === 'whole') {
    return ' is-drop-target'
  }

  let className = ' drop-above'
  if (mark.edge === 'below') {
    className = ' drop-below'
  }
  if (mark.tabGroup !== null) {
    className += ' drop-in-group'
  }
  return className
}
