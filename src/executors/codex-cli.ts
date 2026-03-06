import { relative } from 'node:path'
import { config } from '../config.js'
import { getExternalDirectories } from '../external-dirs.js'
import { getMainWorktreePath } from '../git-worktree.js'
import { runCli } from './cli-runner.js'
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

      const args: string[] = ['exec']
      if (threadId) {
        args.push('resume')
      }
      args.push('--json', '--skip-git-repo-check')
      if (config.codexReasoningEffort) {
        args.push(
          '-c',
          `model_reasoning_effort="${config.codexReasoningEffort}"`,
        )
      }
      // --add-dir is not supported by `codex exec resume`
      if (!threadId) {
        const extraDirs = [
          getMainWorktreePath(),
          ...getExternalDirectories(filePaths),
        ].filter((d): d is string => d !== null)
        for (const dir of extraDirs) {
          args.push('--add-dir', dir)
        }
      }
      args.push('-m', model)
      if (threadId) {
        args.push(threadId)
      }
      args.push(fullPrompt)

      const { stdout, stderr, code } = await runCli('codex', args)

      if (code === 0) {
        const parsed = parseCodexJsonl(stdout)
        if (!parsed.response) {
          throw new Error('No agent_message found in Codex JSONL output')
        }
        return {
          response: parsed.response,
          usage: null,
          threadId: parsed.threadId,
        }
      } else {
        throw new Error(
          `Codex CLI exited with code ${code ?? -1}. Error: ${stderr.trim()}`,
        )
      }
    },
  }
}
