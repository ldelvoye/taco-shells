import type { RuntimeCommand, TerminalInput } from '@protocol/runtimeMessages'
import { serveRuntime } from './server'
import { PtySessions } from './sessions'

const parent = process.parentPort
const sessions = new PtySessions()
const server = serveRuntime(sessions, (report) => {
  parent.postMessage(report)
})

parent.on('message', (event) => {
  if (event.ports.length === 0) {
    const command: RuntimeCommand = event.data
    server.receive(command)
    return
  }

  const port = event.ports[0]
  const send = server.connectWindow((output) => {
    port.postMessage(output)
  })
  port.on('message', (message) => {
    const input: TerminalInput = message.data
    send(input)
  })
  port.start()
})
