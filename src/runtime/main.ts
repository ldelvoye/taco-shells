import type { RuntimeCommand, TerminalRequest } from '@protocol/runtimeMessages'
import { makeXtermScreen } from './engine/xtermScreen'
import { Screens } from './screens'
import { serveRuntime } from './server'
import { PtySessions } from './sessions'

const parent = process.parentPort
const sessions = new PtySessions()
const screens = new Screens({ makeScreen: makeXtermScreen, shells: sessions })
const server = serveRuntime(sessions, screens, (report) => {
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
    const request: TerminalRequest = message.data
    send(request)
  })
  port.start()
})
