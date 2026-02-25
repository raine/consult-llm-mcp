import { spawn } from 'child_process'
import { relative } from 'path'
import { logCliDebug } from '../logger.js'
import type { LlmExecutor } from './types.js'

export function parseGeminiJson(output: string): {
  sessionId: string | undefined
  response: string
} {
  const parsed = JSON.parse(output) as {
    session_id?: string
    response?: string
  }
  return {
    sessionId: parsed.session_id,
    response: parsed.response ?? '',
  }
}

export function createGeminiExecutor(): LlmExecutor {
  const appendFiles = (text: string, filePaths?: string[]): string => {
    if (!filePaths || filePaths.length === 0) return text
    const fileReferences = filePaths
      .map((path) => `@${relative(process.cwd(), path)}`)
      .join(' ')
    return `${text}\n\nFiles: ${fileReferences}`
  }

  return {
    capabilities: {
      isCli: true,
      supportsThreads: true,
      supportsFileRefs: true,
    },

    async execute(prompt, model, systemPrompt, filePaths, threadId) {
      const messageWithFiles = appendFiles(prompt, filePaths)
      const message = threadId
        ? messageWithFiles
        : `${systemPrompt}\n\n${messageWithFiles}`

      const args: string[] = ['-m', model, '-o', 'json']
      if (threadId) {
        args.push('-r', threadId)
      }
      args.push('-p', message)

      return new Promise((resolve, reject) => {
        try {
          logCliDebug('Spawning gemini CLI', {
            model,
            promptLength: message.length,
            threadId,
            args,
          })

          const child = spawn('gemini', args, {
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe'],
          })

          let stdout = ''
          let stderr = ''
          const startTime = Date.now()

          child.on('spawn', () =>
            logCliDebug('gemini CLI process spawned successfully'),
          )
          child.stdout.on('data', (data: Buffer) => (stdout += data.toString()))
          child.stderr.on('data', (data: Buffer) => (stderr += data.toString()))

          child.on('close', (code) => {
            const duration = Date.now() - startTime
            logCliDebug('gemini CLI process closed', {
              code,
              duration: `${duration}ms`,
              stdoutLength: stdout.length,
              stderrLength: stderr.length,
            })

            if (code === 0) {
              try {
                const parsed = parseGeminiJson(stdout)
                if (!parsed.response) {
                  reject(new Error('No response found in Gemini JSON output'))
                  return
                }
                resolve({
                  response: parsed.response,
                  usage: null,
                  threadId: parsed.sessionId,
                })
              } catch {
                logCliDebug('Failed to parse Gemini JSON output', {
                  rawOutput: stdout,
                })
                reject(
                  new Error(
                    `Failed to parse Gemini JSON output: ${stdout.slice(0, 200)}`,
                  ),
                )
              }
            } else {
              if (stderr.includes('RESOURCE_EXHAUSTED')) {
                reject(
                  new Error(
                    `Gemini quota exceeded. Consider using gemini-2.0-flash model. Error: ${stderr.trim()}`,
                  ),
                )
              } else {
                reject(
                  new Error(
                    `Gemini CLI exited with code ${code ?? -1}. Error: ${stderr.trim()}`,
                  ),
                )
              }
            }
          })

          child.on('error', (err) => {
            logCliDebug('Failed to spawn gemini CLI', { error: err.message })
            reject(
              new Error(
                `Failed to spawn gemini CLI. Is it installed and in PATH? Error: ${err.message}`,
              ),
            )
          })
        } catch (err) {
          reject(
            new Error(
              `Synchronous error while trying to spawn gemini: ${
                err instanceof Error ? err.message : String(err)
              }`,
            ),
          )
        }
      })
    },
  }
}
