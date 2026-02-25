import { createInterface } from 'node:readline'
import { spawn } from 'node:child_process'
import { logCliDebug } from '../logger.js'

export interface CliResult {
  stdout: string
  stderr: string
  code: number | null
  duration: number
}

/**
 * Spawn a CLI process and collect its output.
 *
 * Two modes:
 * - **Buffered** (default): collects all stdout into `result.stdout`
 * - **Streaming**: pass `onLine` to process each stdout line as it arrives;
 *   `result.stdout` will be empty
 */
export function runCli(
  command: string,
  args: string[],
  options?: { onLine?: (line: string) => void },
): Promise<CliResult> {
  return new Promise((resolve, reject) => {
    try {
      logCliDebug(`Spawning ${command} CLI`, {
        promptLength: args[args.length - 1]?.length,
        args,
      })

      const child = spawn(command, args, {
        shell: false,
        stdio: ['ignore', 'pipe', 'pipe'],
      })

      let stdout = ''
      let stderr = ''
      const startTime = Date.now()

      child.on('spawn', () =>
        logCliDebug(`${command} CLI process spawned successfully`),
      )

      if (options?.onLine) {
        const rl = createInterface({ input: child.stdout, terminal: false })
        rl.on('line', options.onLine)
      } else {
        child.stdout.setEncoding('utf8')
        child.stdout.on('data', (chunk: string) => (stdout += chunk))
      }

      child.stderr.on('data', (data: Buffer) => (stderr += data.toString()))

      child.on('close', (code) => {
        const duration = Date.now() - startTime
        logCliDebug(`${command} CLI process closed`, {
          code,
          duration: `${duration}ms`,
          stdoutLength: stdout.length,
          stderrLength: stderr.length,
        })
        resolve({ stdout, stderr, code, duration })
      })

      child.on('error', (err) => {
        logCliDebug(`Failed to spawn ${command} CLI`, {
          error: err.message,
        })
        reject(
          new Error(
            `Failed to spawn ${command} CLI. Is it installed and in PATH? Error: ${err.message}`,
          ),
        )
      })
    } catch (err) {
      reject(
        new Error(
          `Synchronous error while trying to spawn ${command}: ${
            err instanceof Error ? err.message : String(err)
          }`,
        ),
      )
    }
  })
}
