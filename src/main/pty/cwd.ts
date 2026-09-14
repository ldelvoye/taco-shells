import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)

// lsof -F tags each line with a single letter for the field it carries, and `n`
// is the one holding the path.
export function parseCwdOutput(output: string): string | null {
  const lines = output.split('\n')
  const cwdLine = lines.find((line) => line.startsWith('n'))
  if (!cwdLine) {
    return null
  }

  const directory = cwdLine.slice(1)
  if (!directory) {
    return null
  }

  return directory
}

export async function processCwd(pid: number): Promise<string | null> {
  try {
    // The only way to learn a running shell's directory without shell integration.
    const { stdout } = await execFileAsync('lsof', ['-a', '-p', String(pid), '-d', 'cwd', '-Fn'], {
      timeout: 1000
    })
    return parseCwdOutput(stdout)
  } catch {
    return null
  }
}
