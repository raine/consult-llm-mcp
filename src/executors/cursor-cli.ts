import { spawn } from 'child_process'
import { relative } from 'path'
import { config } from '../config.js'
import { logCliDebug } from '../logger.js'
import type { LlmExecutor } from './types.js'

export function parseCursorJson(output: string): {
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

export function createCursorExecutor(): LlmExecutor {
  const appendFiles = (text: string, filePaths?: string[]): string => {
    if (!filePaths || filePaths.length === 0) return text
    const fileList = filePaths
      .map((path) => `- ${relative(process.cwd(), path)}`)
      .join('\n')
    return `${text}\n\nPlease read the following files for context:\n${fileList}`
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

      // Map model IDs to cursor-agent model names
      let cursorModel = model.replace('-preview', '')

      // cursor-agent encodes reasoning effort in the model name
      // e.g. gpt-5.3-codex + high → gpt-5.3-codex-high
      if (config.codexReasoningEffort && cursorModel.includes('-codex')) {
        cursorModel = `${cursorModel}-${config.codexReasoningEffort}`
      }

      const args: string[] = [
        '--print',
        '--trust',
        '--output-format',
        'json',
        '--mode',
        'ask',
        '--model',
        cursorModel,
      ]
      if (threadId) {
        args.push('--resume', threadId)
      }
      args.push(message)

      return new Promise((resolve, reject) => {
        try {
          logCliDebug('Spawning cursor-agent CLI', {
            model,
            promptLength: message.length,
            threadId,
            args,
          })

          const child = spawn('cursor-agent', args, {
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe'],
          })

          let stdout = ''
          let stderr = ''
          const startTime = Date.now()

          child.on('spawn', () =>
            logCliDebug('cursor-agent CLI process spawned successfully'),
          )
          child.stdout.on('data', (data: Buffer) => (stdout += data.toString()))
          child.stderr.on('data', (data: Buffer) => (stderr += data.toString()))

          child.on('close', (code) => {
            const duration = Date.now() - startTime
            logCliDebug('cursor-agent CLI process closed', {
              code,
              duration: `${duration}ms`,
              stdoutLength: stdout.length,
              stderrLength: stderr.length,
            })

            if (code === 0) {
              try {
                const parsed = parseCursorJson(stdout)
                if (!parsed.response) {
                  reject(new Error('No result found in Cursor CLI JSON output'))
                  return
                }
                resolve({
                  response: parsed.response,
                  usage: null,
                  threadId: parsed.sessionId ?? threadId,
                })
              } catch {
                logCliDebug('Failed to parse Cursor CLI JSON output', {
                  rawOutput: stdout,
                })
                reject(
                  new Error(
                    `Failed to parse Cursor CLI JSON output: ${stdout.slice(0, 200)}`,
                  ),
                )
              }
            } else {
              reject(
                new Error(
                  `Cursor CLI exited with code ${code ?? -1}. Error: ${stderr.trim()}`,
                ),
              )
            }
          })

          child.on('error', (err) => {
            logCliDebug('Failed to spawn cursor-agent CLI', {
              error: err.message,
            })
            reject(
              new Error(
                `Failed to spawn cursor-agent CLI. Is it installed and in PATH? Error: ${err.message}`,
              ),
            )
          })
        } catch (err) {
          reject(
            new Error(
              `Synchronous error while trying to spawn cursor-agent: ${
                err instanceof Error ? err.message : String(err)
              }`,
            ),
          )
        }
      })
    },
  }
}
