const SWITCH_TIMEOUT_MS = 5000
const ECHO_TIMEOUT_MS = 10000

export const PAGE_PROBE = `(() => {
  const bytes = {}
  const dataListeners = new Set()
  const textWaiters = []
  const state = { loading: false, lastFrame: 0, frameGaps: [], longTasks: 0, observer: null, hidden: false }

  window.tacoShells.pty.onEvent((event) => {
    let data = null
    if (event.type === 'output') {
      data = event.data
    }
    if (event.type === 'attached') {
      data = event.snapshot
    }
    if (data === null) {
      return
    }

    const id = event.id
    let before = 0
    if (bytes[id]) {
      before = bytes[id]
    }
    bytes[id] = before + data.length

    for (const waiter of [...textWaiters]) {
      if (waiter.id !== id) {
        continue
      }
      const combined = waiter.tail + data
      if (combined.includes(waiter.text)) {
        textWaiters.splice(textWaiters.indexOf(waiter), 1)
        waiter.resolve()
        continue
      }
      waiter.tail = combined.slice(-waiter.text.length)
    }

    for (const listener of [...dataListeners]) {
      listener(id)
    }
  })

  document.addEventListener('webglcontextlost', () => {
    api.contextsLost += 1
  }, true)

  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') {
      state.hidden = true
    }
  })

  function nextFrame() {
    return new Promise((resolve) => {
      requestAnimationFrame(() => resolve())
    })
  }

  function nextData(id) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        dataListeners.delete(listener)
        reject(new Error('no output arrived from ' + id))
      }, ${ECHO_TIMEOUT_MS})
      function listener(dataId) {
        if (dataId !== id) {
          return
        }
        clearTimeout(timer)
        dataListeners.delete(listener)
        resolve()
      }
      dataListeners.add(listener)
    })
  }

  function visibleGroup(area) {
    const groups = [...area.querySelectorAll('.pane-group')]
    return groups.find((group) => !group.classList.contains('is-inactive'))
  }

  function tick(now) {
    if (!state.loading) {
      return
    }
    if (state.lastFrame > 0) {
      state.frameGaps.push(now - state.lastFrame)
    }
    state.lastFrame = now
    requestAnimationFrame(tick)
  }

  const api = {
    bytes,
    contextsLost: 0,
    switched: Promise.resolve(0),
    keyed: Promise.resolve(0),

    startLoad() {
      state.frameGaps = []
      state.longTasks = 0
      state.lastFrame = 0
      state.hidden = document.visibilityState === 'hidden'
      state.loading = true
      state.observer = new PerformanceObserver((list) => {
        state.longTasks += list.getEntries().length
      })
      state.observer.observe({ entryTypes: ['longtask'] })
      requestAnimationFrame(tick)
    },

    stopLoad() {
      state.loading = false
      const leftover = state.observer.takeRecords()
      state.longTasks += leftover.length
      state.observer.disconnect()
      return { frameGaps: state.frameGaps, longTasks: state.longTasks, hidden: state.hidden }
    },

    armKey(id) {
      const pressed = new Promise((resolve) => {
        window.addEventListener('keydown', (event) => {
          resolve(event.timeStamp)
        }, { capture: true, once: true })
      })
      const echoed = nextData(id)
      async function drawn() {
        await echoed
        await nextFrame()
        const drawnAt = performance.now()
        const pressedAt = await pressed
        return drawnAt - pressedAt
      }
      api.keyed = drawn()
    },

    waitForText(id, text) {
      return new Promise((resolve) => {
        textWaiters.push({ id, text, tail: '', resolve })
      })
    },

    quietAfter(id, quietMs) {
      return new Promise((resolve) => {
        const start = performance.now()
        let last = start
        let timer = null
        function finish() {
          dataListeners.delete(listener)
          resolve(last - start)
        }
        function listener(dataId) {
          if (dataId !== id) {
            return
          }
          last = performance.now()
          clearTimeout(timer)
          timer = setTimeout(finish, quietMs)
        }
        dataListeners.add(listener)
        timer = setTimeout(finish, quietMs)
      })
    },

    armSwitch() {
      const area = document.querySelector('.pane-area')
      const before = visibleGroup(area)
      const keyed = new Promise((resolve) => {
        window.addEventListener('keydown', (event) => {
          resolve(event.timeStamp)
        }, { capture: true, once: true })
      })

      api.switched = new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          watcher.disconnect()
          reject(new Error('no switch happened'))
        }, ${SWITCH_TIMEOUT_MS})
        const watcher = new MutationObserver(async () => {
          const now = visibleGroup(area)
          if (!now || now === before) {
            return
          }
          watcher.disconnect()
          clearTimeout(timer)
          const keyAt = await keyed
          await nextFrame()
          await nextFrame()
          resolve(performance.now() - keyAt)
        })
        watcher.observe(area, { attributes: true, childList: true, subtree: true })
      })
    },

    onSlowRenderer() {
      const hosts = [...document.querySelectorAll('.terminal-host')]
      const slow = hosts.filter((host) => host.querySelector('.xterm-rows') !== null)
      return slow.length
    }
  }

  window.__measure = api
})()`
