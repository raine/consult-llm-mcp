import OpenAI from 'openai'
import { spawn } from 'child_process'
import { relative } from 'path'
import { config } from './config.js'
import { type SupportedChatModel as SupportedChatModelType } from './schema.js'
import { logCliDebug } from './logger.js'

export interface LlmExecutor {
  execute(
    prompt: string,
    model: SupportedChatModelType,
    systemPrompt: string,
    filePaths?: string[],
    threadId?: string,
  ): Promise<{
    response: string
    usage: OpenAI.CompletionUsage | null
    threadId?: string
  }>
}

/**
 * Creates an executor that interacts with an OpenAI-compatible API.
 *
 * Don't let it confuse you that client is of type OpenAI. We used OpenAI API
 * client for Gemini also.
 */
function createApiExecutor(client: OpenAI): LlmExecutor {
  return {
    async execute(prompt, model, systemPrompt, filePaths) {
      if (filePaths && filePaths.length > 0) {
        // Explicitly reject unsupported parameters
        console.warn(
          `Warning: File paths were provided but are not supported by the API executor for model ${model}. They will be ignored.`,
        )
      }

      const completion = await client.chat.completions.create({
        model,
        messages: [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: prompt },
        ],
      })

      const response = completion.choices[0]?.message?.content
      if (!response) {
        throw new Error('No response from the model via API')
      }

      return { response, usage: completion.usage ?? null }
    },
  }
}

// --- CLI Executors ---

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

function createGeminiExecutor(): LlmExecutor {
  const buildFullPrompt = (
    prompt: string,
    systemPrompt: string,
    filePaths?: string[],
  ): string => {
    let fullPrompt = `${systemPrompt}\n\n${prompt}`
    if (filePaths && filePaths.length > 0) {
      const fileReferences = filePaths
        .map((path) => `@${relative(process.cwd(), path)}`)
        .join(' ')
      fullPrompt = `${fullPrompt}\n\nFiles: ${fileReferences}`
    }
    return fullPrompt
  }

  return {
    async execute(prompt, model, systemPrompt, filePaths, threadId) {
      const message = threadId
        ? prompt
        : buildFullPrompt(prompt, systemPrompt, filePaths)

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

function createCodexExecutor(): LlmExecutor {
  const appendFiles = (text: string, filePaths?: string[]): string => {
    if (!filePaths || filePaths.length === 0) return text
    const fileRefs = filePaths
      .map((path) => `@${relative(process.cwd(), path)}`)
      .join(' ')
    return `${text}\n\nFiles: ${fileRefs}`
  }

  return {
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

const createExecutorProvider = () => {
  const executorCache = new Map<string, LlmExecutor>()
  const clientCache = new Map<string, OpenAI>()

  const getOpenAIClient = (): OpenAI => {
    if (clientCache.has('openai')) return clientCache.get('openai')!
    if (!config.openaiApiKey) {
      throw new Error(
        'OPENAI_API_KEY environment variable is required for OpenAI models in API mode',
      )
    }
    const client = new OpenAI({ apiKey: config.openaiApiKey })
    clientCache.set('openai', client)
    return client
  }

  const getDeepseekClient = (): OpenAI => {
    if (clientCache.has('deepseek')) return clientCache.get('deepseek')!
    if (!config.deepseekApiKey) {
      throw new Error(
        'DEEPSEEK_API_KEY environment variable is required for DeepSeek models',
      )
    }
    const client = new OpenAI({
      apiKey: config.deepseekApiKey,
      baseURL: 'https://api.deepseek.com',
    })
    clientCache.set('deepseek', client)
    return client
  }

  const getGeminiApiClient = (): OpenAI => {
    if (clientCache.has('geminiApi')) return clientCache.get('geminiApi')!
    if (!config.geminiApiKey) {
      throw new Error(
        'GEMINI_API_KEY environment variable is required for Gemini models in API mode',
      )
    }
    const client = new OpenAI({
      apiKey: config.geminiApiKey,
      baseURL: 'https://generativelanguage.googleapis.com/v1beta/openai/',
    })
    clientCache.set('geminiApi', client)
    return client
  }

  return (model: SupportedChatModelType): LlmExecutor => {
    // Create cache key that includes mode for models that support CLI
    const cacheKey =
      model +
      (model.startsWith('gpt-') ? `-${config.openaiMode}` : '') +
      (model.startsWith('gemini-') ? `-${config.geminiMode}` : '')

    if (executorCache.has(cacheKey)) {
      return executorCache.get(cacheKey)!
    }

    let executor: LlmExecutor

    if (model.startsWith('gpt-')) {
      executor =
        config.openaiMode === 'cli'
          ? createCodexExecutor()
          : createApiExecutor(getOpenAIClient())
    } else if (model.startsWith('deepseek-')) {
      executor = createApiExecutor(getDeepseekClient())
    } else if (model.startsWith('gemini-')) {
      executor =
        config.geminiMode === 'cli'
          ? createGeminiExecutor()
          : createApiExecutor(getGeminiApiClient())
    } else {
      throw new Error(`Unable to determine LLM provider for model: ${model}`)
    }

    executorCache.set(cacheKey, executor)
    return executor
  }
}

export const getExecutorForModel = createExecutorProvider()
