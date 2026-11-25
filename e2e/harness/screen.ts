import { evaluate, type Protocol } from './connection.ts'
import { POLL_INTERVAL_MS, sleep } from './poll.ts'

const SETTLE_TIMEOUT_MS = 15000

// The sidebar's rows and the on-screen group's panes are two separate lists:
// rows cover every group, panes only the one not hidden.
const SCREEN_SCRIPT = `JSON.stringify({
  rows: [...document.querySelectorAll('.terminal-row')].map((row) => {
    const connector = row.querySelector('.row-connector')
    let piece = ''
    if (connector) {
      piece = connector.className.replace('row-connector ', '')
    }
    const stripe = row.querySelector('.row-color')
    let color = null
    if (stripe) {
      color = stripe.dataset.color
    }
    return {
      label: row.querySelector('.row-label').textContent,
      connector: piece,
      active: row.classList.contains('is-active'),
      color,
      grouped: row.closest('.tab-group') !== null
    }
  }),
  headers: [...document.querySelectorAll('.tab-group-header')].map((header) => {
    const count = header.querySelector('.tab-group-count')
    let countValue = null
    if (count) {
      countValue = Number(count.textContent)
    }
    const marker = header.querySelector('.row-state')
    let state = null
    if (marker && marker.dataset.state) {
      state = marker.dataset.state
    }
    return {
      name: header.querySelector('.tab-group-name').textContent,
      collapsed: header.parentElement.classList.contains('is-collapsed'),
      count: countValue,
      state,
      active: header.classList.contains('is-active')
    }
  }),
  panes: [...document.querySelectorAll('.pane-group:not(.is-inactive) .terminal-view')].map((view) => ({
    width: Number(view.style.flexGrow),
    pixels: Math.round(view.getBoundingClientRect().width)
  })),
  groups: document.querySelectorAll('.pane-group').length,
  dividers: document.querySelectorAll('.pane-group:not(.is-inactive) .pane-divider').length,
  sidebarVisible: document.querySelector('.sidebar') !== null,
  sidebarWidth: (() => {
    const sidebar = document.querySelector('.sidebar')
    if (!sidebar) {
      return 0
    }
    return Math.round(sidebar.getBoundingClientRect().width)
  })(),
  windowWidth: window.innerWidth,
  // A range rather than the usual scrollWidth against clientWidth: both of those
  // are rounded, so they read equal while the label is a fraction over and still
  // showing an ellipsis.
  labelsTruncated: (() => {
    const labels = [...document.querySelectorAll('.row-label')]
    return labels.some((label) => {
      const range = document.createRange()
      range.selectNodeContents(label)
      const textWidth = range.getBoundingClientRect().width
      const availableWidth = label.getBoundingClientRect().width
      return textWidth > availableWidth
    })
  })(),
  colorPickerOpen: document.querySelector('.popup') !== null,
  forkItem: (() => {
    const item = document.querySelector('.popup-item')
    if (!item) {
      return 'absent'
    }
    if (item.disabled) {
      return 'disabled'
    }
    return 'enabled'
  })(),
  terminalFocused: document.querySelector('.xterm-helper-textarea:focus') !== null,
  nameFieldFocused: document.querySelector('.tab-group-name-field:focus') !== null
})`

/** A sidebar row: every pane of every group has one, in sidebar order. */
export interface Row {
  label: string
  connector: string
  active: boolean
  color: string | null
  grouped: boolean
}

/** A tab group's header: every tab group has one, in sidebar order. */
export interface Header {
  name: string
  collapsed: boolean
  count: number | null
  state: string | null
  active: boolean
}

/** A pane of the group currently on screen, left to right. */
export interface VisiblePane {
  width: number
  pixels: number
}

export interface Screen {
  rows: Row[]
  headers: Header[]
  panes: VisiblePane[]
  groups: number
  dividers: number
  sidebarVisible: boolean
  sidebarWidth: number
  windowWidth: number
  labelsTruncated: boolean
  colorPickerOpen: boolean
  forkItem: 'absent' | 'enabled' | 'disabled'
  terminalFocused: boolean
  nameFieldFocused: boolean
}

export async function readScreen(page: Protocol): Promise<Screen> {
  const raw = await evaluate(page, SCREEN_SCRIPT)
  return JSON.parse(raw) as Screen
}

export async function until(
  page: Protocol,
  description: string,
  holds: (screen: Screen) => boolean
): Promise<Screen> {
  const deadline = Date.now() + SETTLE_TIMEOUT_MS
  let latest = await readScreen(page)

  while (Date.now() < deadline) {
    if (holds(latest)) {
      return latest
    }
    await sleep(POLL_INTERVAL_MS)
    latest = await readScreen(page)
  }

  throw new Error(`timed out waiting until ${description}; last saw ${JSON.stringify(latest)}`)
}
