const LAG_INTERVAL_MS = 5

export const START_LAG = `(() => {
  const state = { samples: [], expected: performance.now() +${LAG_INTERVAL_MS}, timer: null }
  state.timer = setInterval(() => {
    const now = performance.now()
    const late = Math.max(0, now - state.expected)
    state.samples.push(late)
    state.expected = now + ${LAG_INTERVAL_MS}
  }, ${LAG_INTERVAL_MS})
  globalThis.__measureLag = state
  return ''
})()`

export const STOP_LAG = `(() => {
  const state = globalThis.__measureLag
  clearInterval(state.timer)
  return JSON.stringify(state.samples)
})()`

export const MEMORY = `(() => {
  const { app } = process.mainModule.require('electron')
  return JSON.stringify(app.getAppMetrics())
})()`
