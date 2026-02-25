import { spawn } from 'child_process'
import { relative } from 'path'
import { logCliDebug } from '../logger.js'
import type { LlmExecutor } from './types.js'

export function parseAgentJson(output: string): {
  sessionId: string | undefined
  response: string
} {
  const parsed = JSON.parse(output) as {
    session_id?: string
    result?: string
  }
  return {
    sessionId: parsed.session_id,
    response: parsed.result ?? '',
  }
}

export function createAgentExecutor(): LlmExecutor {
  const buildFullPrompt = (
    prompt: string,
    systemPrompt: string,
    filePaths?: string[],
  ): string => {
    let fullPrompt = `${systemPrompt}\n\n${prompt}`
    if (filePaths && filePaths.length > 0) {
      const fileList = filePaths
        .map((path) => `- ${relative(process.cwd(), path)}`)
        .join('\n')
      fullPrompt = `${fullPrompt}\n\nPlease read the following files for context:\n${fileList}`
    }
    return fullPrompt
  }

  return {
    capabilities: {
      isCli: true,
      supportsThreads: true,
      supportsFileRefs: true,
    },

    async execute(prompt, model, systemPrompt, filePaths, threadId) {
      const message = threadId
        ? prompt
        : buildFullPrompt(prompt, systemPrompt, filePaths)

      const args: string[] = [
        '--print',
        '--trust',
        '--output-format',
        'json',
        '--mode',
        'ask',
        '--model',
        model,
      ]
      if (threadId) {
        args.push('--resume', threadId)
      }
      args.push(message)

      return new Promise((resolve, reject) => {
        try {
          logCliDebug('Spawning agent CLI', {
            model,
            promptLength: message.length,
            threadId,
            args,
          })

          const child = spawn('agent', args, {
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe'],
          })

          let stdout = ''
          let stderr = ''
          const startTime = Date.now()

          child.on('spawn', () =>
            logCliDebug('agent CLI process spawned successfully'),
          )
          child.stdout.on('data', (data: Buffer) => (stdout += data.toString()))
          child.stderr.on('data', (data: Buffer) => (stderr += data.toString()))

          child.on('close', (code) => {
            const duration = Date.now() - startTime
            logCliDebug('agent CLI process closed', {
              code,
              duration: `${duration}ms`,
              stdoutLength: stdout.length,
              stderrLength: stderr.length,
            })

            if (code === 0) {
              try {
                const parsed = parseAgentJson(stdout)
                if (!parsed.response) {
                  reject(new Error('No result found in Agent CLI JSON output'))
                  return
                }
                resolve({
                  response: parsed.response,
                  usage: null,
                  threadId: parsed.sessionId,
                })
              } catch {
                logCliDebug('Failed to parse Agent CLI JSON output', {
                  rawOutput: stdout,
                })
                reject(
                  new Error(
                    `Failed to parse Agent CLI JSON output: ${stdout.slice(0, 200)}`,
                  ),
                )
              }
            } else {
              reject(
                new Error(
                  `Agent CLI exited with code ${code ?? -1}. Error: ${stderr.trim()}`,
                ),
              )
            }
          })

          child.on('error', (err) => {
            logCliDebug('Failed to spawn agent CLI', { error: err.message })
            reject(
              new Error(
                `Failed to spawn agent CLI. Is it installed and in PATH? Error: ${err.message}`,
              ),
            )
          })
        } catch (err) {
          reject(
            new Error(
              `Synchronous error while trying to spawn agent: ${
                err instanceof Error ? err.message : String(err)
              }`,
            ),
          )
        }
      })
    },
  }
}
