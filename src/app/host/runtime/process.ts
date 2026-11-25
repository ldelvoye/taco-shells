import { join } from 'node:path'
import { MessageChannelMain, utilityProcess, type WebContents } from 'electron'
import { CHANNEL } from '@protocol/ipc'
import type { Runtime } from '@protocol/runtime'
import type { RuntimeReport } from '@protocol/runtimeMessages'
import { createRuntimeProxy } from './proxy'

const RUNTIME_SCRIPT = 'runtime.js'
const SERVICE_NAME = 'Terminal runtime'
const WINDOW_PORT_MESSAGE = 'window'

export interface RuntimeProcess {
  runtime: Runtime
  /** A fresh connection for a window that has just loaded. */
  connectWindow(sender: WebContents): void
  onDied(listener: (reason: string) => void): void
  stop(): void
}

export function startRuntimeProcess(): RuntimeProcess {
  const script = join(__dirname, RUNTIME_SCRIPT)
  const child = utilityProcess.fork(script, [], { serviceName: SERVICE_NAME })
  const diedListeners: ((reason: string) => void)[] = []
  let alive = true
  let stopping = false

  const proxy = createRuntimeProxy((command) => {
    if (alive) {
      child.postMessage(command)
    }
  })

  child.on('message', (message) => {
    const report: RuntimeReport = message
    proxy.receive(report)
  })

  child.on('exit', (code) => {
    alive = false
    proxy.disconnect()
    if (stopping) {
      return
    }
    const reason = `code ${code}`
    for (const listener of diedListeners) {
      listener(reason)
    }
  })

  return {
    runtime: proxy,
    connectWindow(sender: WebContents): void {
      if (!alive) {
        return
      }
      const channel = new MessageChannelMain()
      child.postMessage(WINDOW_PORT_MESSAGE, [channel.port1])
      sender.postMessage(CHANNEL.runtimePort, null, [channel.port2])
    },
    onDied(listener: (reason: string) => void): void {
      diedListeners.push(listener)
    },
    stop(): void {
      stopping = true
      if (alive) {
        child.kill()
      }
    }
  }
}
