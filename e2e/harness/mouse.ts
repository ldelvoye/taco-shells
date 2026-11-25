import { evaluate, type Protocol, type ProtocolParams } from './connection.ts'
import { pollFor } from './poll.ts'

const DRAG_STEPS = 8
const DRAG_START_TIMEOUT_MS = 3000
// Pixels in from a row's left edge: over its label, and on the bracket's stem (--bracket-stem in app.css).
export const ROW_HANDLE_OFFSET = 60

export interface Point {
  x: number
  y: number
}

export interface RowBox {
  left: number
  top: number
  height: number
}

export async function centreOf(page: Protocol, selector: string, index: number): Promise<Point> {
  const centreScript = `JSON.stringify((() => {
      const handles = document.querySelectorAll('${selector}')
      const box = handles[${index}].getBoundingClientRect()
      return { x: Math.round(box.left + box.width / 2), y: Math.round(box.top + box.height / 2) }
    })())`
  const measured = await evaluate(page, centreScript)
  return JSON.parse(measured) as Point
}

export async function boxOf(page: Protocol, selector: string, index: number): Promise<RowBox> {
  const boxScript = `JSON.stringify((() => {
      const elements = document.querySelectorAll('${selector}')
      const box = elements[${index}].getBoundingClientRect()
      return { left: box.left, top: box.top, height: box.height }
    })())`
  const measured = await evaluate(page, boxScript)
  return JSON.parse(measured) as RowBox
}

export async function rowBox(page: Protocol, rowIndex: number): Promise<RowBox> {
  return boxOf(page, '.terminal-row', rowIndex)
}

export async function clickAt(
  page: Protocol,
  centre: Point,
  button: 'left' | 'right'
): Promise<void> {
  let buttons = 1
  if (button === 'right') {
    buttons = 2
  }
  const clicked = { x: centre.x, y: centre.y, button, buttons, clickCount: 1 }

  await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...clicked })
  await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...clicked })
}

export async function hover(page: Protocol, selector: string, index: number): Promise<Point> {
  const centre = await centreOf(page, selector, index)
  await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: centre.x, y: centre.y })
  return centre
}

// The button has no box until its row or header is hovered, and there is one per row or header.
export async function clickButtonWithin(
  page: Protocol,
  selector: string,
  index: number,
  button: string
): Promise<void> {
  await hover(page, selector, index)
  const centre = await hover(page, button, index)
  await clickAt(page, centre, 'left')
}

export async function dragAcross(
  page: Protocol,
  selector: string,
  index: number,
  byPixels: number
): Promise<void> {
  const centre = await centreOf(page, selector, index)

  const held = { button: 'left', buttons: 1, clickCount: 1 }
  const destinationX = centre.x + byPixels

  await page.send('Input.dispatchMouseEvent', {
    type: 'mousePressed',
    x: centre.x,
    y: centre.y,
    ...held
  })
  await page.send('Input.dispatchMouseEvent', {
    type: 'mouseMoved',
    x: destinationX,
    y: centre.y,
    ...held
  })
  await page.send('Input.dispatchMouseEvent', {
    type: 'mouseReleased',
    x: destinationX,
    y: centre.y,
    ...held
  })
}

export async function rightClickRow(page: Protocol, rowIndex: number): Promise<void> {
  const centre = await centreOf(page, '.terminal-row', rowIndex)
  await clickAt(page, centre, 'right')
}

export async function clickColorDot(page: Protocol, color: string): Promise<void> {
  const centre = await hover(page, `.color-dot[data-color=${color}]`, 0)
  await clickAt(page, centre, 'left')
}

/** HTML5 drags in the sidebar, which Chromium hands over as data to be dropped by hand. */
export class SidebarDrags {
  private readonly page: Protocol
  private intercepted: ProtocolParams | null = null

  constructor(page: Protocol) {
    this.page = page
    page.onEvent('Input.dragIntercepted', (params) => {
      this.intercepted = params.data as ProtocolParams
    })
  }

  async drag(source: Point, target: Point): Promise<void> {
    this.intercepted = null
    const held = { button: 'left', buttons: 1, clickCount: 1 }

    await this.page.send('Input.setInterceptDrags', { enabled: true })
    await this.page.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...source, ...held })

    let step = 1
    while (step <= DRAG_STEPS) {
      const progress = step / DRAG_STEPS
      const x = source.x + (target.x - source.x) * progress
      const y = source.y + (target.y - source.y) * progress
      await this.page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, ...held })
      step += 1
    }

    const data = await pollFor(
      'the drag to start',
      async () => this.intercepted,
      DRAG_START_TIMEOUT_MS
    )

    await this.page.send('Input.dispatchDragEvent', { type: 'dragEnter', ...target, data })
    await this.page.send('Input.dispatchDragEvent', { type: 'dragOver', ...target, data })
    await this.page.send('Input.dispatchDragEvent', { type: 'drop', ...target, data })
    await this.page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...target, ...held })
    await this.page.send('Input.setInterceptDrags', { enabled: false })
  }
}
