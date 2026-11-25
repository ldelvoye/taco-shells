import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)

const PS_ROW = /^\s*(\d+)\s+(\d+)\s+(-?\d+)\s+(.+)$/
const PS_MAX_BUFFER_BYTES = 16 * 1024 * 1024

interface ProcessRow {
  pid: number
  pgid: number
  tpgid: number
  command: string
}

function parseRow(line: string): ProcessRow | null {
  const match = PS_ROW.exec(line)
  if (match === null) {
    return null
  }
  return {
    pid: Number(match[1]),
    pgid: Number(match[2]),
    tpgid: Number(match[3]),
    command: match[4].trim()
  }
}

/** The command lines in the foreground process group of a shell's terminal, from `ps -axo pid=,pgid=,tpgid=,command=` output. */
export function parseForegroundCommands(output: string, shellPid: number): string[] {
  const lines = output.split('\n')
  const rows: ProcessRow[] = []
  for (const line of lines) {
    const row = parseRow(line)
    if (row !== null) {
      rows.push(row)
    }
  }

  const shell = rows.find((row) => row.pid === shellPid)
  if (shell === undefined) {
    return []
  }

  const foreground = rows.filter((row) => row.pgid === shell.tpgid)
  return foreground.map((row) => row.command)
}

/** The foreground command lines of a shell's terminal, or none when `ps` fails. */
export async function foregroundCommands(shellPid: number): Promise<string[]> {
  try {
    const { stdout } = await execFileAsync('ps', ['-axo', 'pid=,pgid=,tpgid=,command='], {
      timeout: 1000,
      maxBuffer: PS_MAX_BUFFER_BYTES
    })
    return parseForegroundCommands(stdout, shellPid)
  } catch {
    return []
  }
}
