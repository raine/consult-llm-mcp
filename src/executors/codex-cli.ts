import { spawn } from 'node:child_process'
import { relative } from 'node:path'
import { config } from '../config.js'
import { logCliDebug } from '../logger.js'
import type { LlmExecutor } from './types.js'

export function parseCodexJsonl(output: string): {
  threadId: string | undefined
  response: string
} {
  let threadId: string | undefined
  const messages: string[] = []

  for (const line of output.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    try {
      const event = JSON.parse(trimmed) as {
        type?: string
        thread_id?: string
        item?: { type?: string; text?: string }
      }
      if (event.type === 'thread.started' && event.thread_id) {
        threadId = event.thread_id
      } else if (
        event.type === 'item.completed' &&
        event.item?.type === 'agent_message' &&
        event.item?.text
      ) {
        messages.push(event.item.text)
      }
    } catch {
      // Skip non-JSON lines (e.g. the ERROR log from resume)
    }
  }

  return { threadId, response: messages.join('\n') }
}

export function createCodexExecutor(): LlmExecutor {
  const appendFiles = (text: string, filePaths?: string[]): string => {
    if (!filePaths || filePaths.length === 0) return text
    const fileRefs = filePaths
      .map((path) => `@${relative(process.cwd(), path)}`)
      .join(' ')
    return `${text}\n\nFiles: ${fileRefs}`
  }

  return {
    capabilities: {
      isCli: true,
      supportsThreads: true,
      supportsFileRefs: true,
    },

    async execute(prompt, model, systemPrompt, filePaths, threadId) {
      const message = appendFiles(prompt, filePaths)
      const fullPrompt = threadId
        ? message // On resume, include files but skip system prompt
        : `${systemPrompt}\n\n${message}`

      const args: string[] = []
      if (threadId) {
        args.push('exec', 'resume', '--json', '--skip-git-repo-check')
        if (config.codexReasoningEffort) {
          args.push(
            '-c',
            `model_reasoning_effort="${config.codexReasoningEffort}"`,
          )
        }
        args.push('-m', model, threadId, fullPrompt)
      } else {
        args.push('exec', '--json', '--skip-git-repo-check')
        if (config.codexReasoningEffort) {
          args.push(
            '-c',
            `model_reasoning_effort="${config.codexReasoningEffort}"`,
          )
        }
        args.push('-m', model, fullPrompt)
      }

      return new Promise((resolve, reject) => {
        try {
          logCliDebug('Spawning codex CLI', {
            model,
            promptLength: fullPrompt.length,
            threadId,
            args,
          })

          const child = spawn('codex', args, {
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe'],
          })

          let stdout = ''
          let stderr = ''
          const startTime = Date.now()

          child.on('spawn', () =>
            logCliDebug('codex CLI process spawned successfully'),
          )
          child.stdout.on('data', (data: Buffer) => (stdout += data.toString()))
          child.stderr.on('data', (data: Buffer) => (stderr += data.toString()))

          child.on('close', (code) => {
            const duration = Date.now() - startTime
            logCliDebug('codex CLI process closed', {
              code,
              duration: `${duration}ms`,
              stdoutLength: stdout.length,
              stderrLength: stderr.length,
            })

            if (code === 0) {
              const parsed = parseCodexJsonl(stdout)
              if (!parsed.response) {
                reject(
                  new Error('No agent_message found in Codex JSONL output'),
                )
                return
              }
              resolve({
                response: parsed.response,
                usage: null,
                threadId: parsed.threadId,
              })
            } else {
              reject(
                new Error(
                  `Codex CLI exited with code ${code ?? -1}. Error: ${stderr.trim()}`,
                ),
              )
            }
          })

          child.on('error', (err) => {
            logCliDebug('Failed to spawn codex CLI', { error: err.message })
            reject(
              new Error(
                `Failed to spawn codex CLI. Is it installed and in PATH? Error: ${err.message}`,
              ),
            )
          })
        } catch (err) {
          reject(
            new Error(
              `Synchronous error while trying to spawn codex: ${
                err instanceof Error ? err.message : String(err)
              }`,
            ),
          )
        }
      })
    },
  }
}
