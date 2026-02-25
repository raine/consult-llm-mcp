import { relative } from 'node:path'
import { logCliDebug } from '../logger.js'
import { runCli } from './cli-runner.js'
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

      const { stdout, stderr, code } = await runCli('gemini', args)

      if (code === 0) {
        try {
          const parsed = parseGeminiJson(stdout)
          if (!parsed.response) {
            throw new Error('No response found in Gemini JSON output')
          }
          return {
            response: parsed.response,
            usage: null,
            threadId: parsed.sessionId,
          }
        } catch (err) {
          if (err instanceof Error && err.message.includes('No response')) {
            throw err
          }
          logCliDebug('Failed to parse Gemini JSON output', {
            rawOutput: stdout,
          })
          throw new Error(
            `Failed to parse Gemini JSON output: ${stdout.slice(0, 200)}`,
          )
        }
      } else {
        if (stderr.includes('RESOURCE_EXHAUSTED')) {
          throw new Error(
            `Gemini quota exceeded. Consider using gemini-2.0-flash model. Error: ${stderr.trim()}`,
          )
        }
        throw new Error(
          `Gemini CLI exited with code ${code ?? -1}. Error: ${stderr.trim()}`,
        )
      }
    },
  }
}
