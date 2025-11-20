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
  ): Promise<{
    response: string
    usage: OpenAI.CompletionUsage | null
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

/**
 * Configuration for a command-line tool executor.
 */
type CliConfig = {
  cliName: string
  buildArgs: (model: SupportedChatModelType, fullPrompt: string) => string[]
  handleNonZeroExit: (code: number, stderr: string) => Error
}

/**
 * Creates an executor that delegates to a command-line tool.
 */
function createCliExecutor(cliConfig: CliConfig): LlmExecutor {
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
    async execute(prompt, model, systemPrompt, filePaths) {
      const fullPrompt = buildFullPrompt(prompt, systemPrompt, filePaths)
      const args = cliConfig.buildArgs(model, fullPrompt)
      const { cliName } = cliConfig

      return new Promise((resolve, reject) => {
        try {
          logCliDebug(`Spawning ${cliName} CLI`, {
            model,
            promptLength: fullPrompt.length,
            filePathsCount: filePaths?.length || 0,
            args: args,
            promptPreview: fullPrompt.slice(0, 300),
          })

          const child = spawn(cliName, args, {
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe'],
          })

          let stdout = ''
          let stderr = ''
          let hasResponded = false
          const startTime = Date.now()

          child.on('spawn', () =>
            logCliDebug(`${cliName} CLI process spawned successfully`),
          )

          const timeout = setTimeout(
            () => {
              if (!hasResponded) {
                logCliDebug(`${cliName} CLI timed out after 5 minutes`)
                child.kill()
                reject(new Error(`${cliName} CLI timed out after 5 minutes`))
              }
            },
            5 * 60 * 1000,
          )

          child.stdout.on('data', (data: Buffer) => (stdout += data.toString()))
          child.stderr.on('data', (data: Buffer) => (stderr += data.toString()))

          child.on('close', (code) => {
            hasResponded = true
            clearTimeout(timeout)
            const duration = Date.now() - startTime

            logCliDebug(`${cliName} CLI process closed`, {
              code,
              duration: `${duration}ms`,
              stdoutLength: stdout.length,
              stderrLength: stderr.length,
            })

            if (code === 0) {
              resolve({ response: stdout.trim(), usage: null })
            } else {
              reject(cliConfig.handleNonZeroExit(code ?? -1, stderr))
            }
          })

          child.on('error', (err) => {
            hasResponded = true
            clearTimeout(timeout)
            logCliDebug(`Failed to spawn ${cliName} CLI`, {
              error: err.message,
            })
            reject(
              new Error(
                `Failed to spawn ${cliName} CLI. Is it installed and in PATH? Error: ${err.message}`,
              ),
            )
          })
        } catch (err) {
          reject(
            new Error(
              `Synchronous error while trying to spawn ${cliName}: ${
                err instanceof Error ? err.message : String(err)
              }`,
            ),
          )
        }
      })
    },
  }
}

// --- CLI Configurations ---
const geminiCliConfig: CliConfig = {
  cliName: 'gemini',
  buildArgs: (model, fullPrompt) => ['-m', model, '-p', fullPrompt],
  handleNonZeroExit: (code, stderr) => {
    if (stderr.includes('RESOURCE_EXHAUSTED')) {
      return new Error(
        `Gemini quota exceeded. Consider using gemini-2.0-flash model. Error: ${stderr.trim()}`,
      )
    }
    return new Error(
      `Gemini CLI exited with code ${code}. Error: ${stderr.trim()}`,
    )
  },
}

const codexCliConfig: CliConfig = {
  cliName: 'codex',
  buildArgs: (model, fullPrompt) => ['exec', '-m', model, fullPrompt],
  handleNonZeroExit: (code, stderr) =>
    new Error(`Codex CLI exited with code ${code}. Error: ${stderr.trim()}`),
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
      (model.startsWith('gpt-') || model === 'o3'
        ? `-${config.openaiMode}`
        : '') +
      (model.startsWith('gemini-') ? `-${config.geminiMode}` : '')

    if (executorCache.has(cacheKey)) {
      return executorCache.get(cacheKey)!
    }

    let executor: LlmExecutor

    if (model.startsWith('gpt-') || model === 'o3') {
      executor =
        config.openaiMode === 'cli'
          ? createCliExecutor(codexCliConfig)
          : createApiExecutor(getOpenAIClient())
    } else if (model.startsWith('deepseek-')) {
      executor = createApiExecutor(getDeepseekClient())
    } else if (model.startsWith('gemini-')) {
      executor =
        config.geminiMode === 'cli'
          ? createCliExecutor(geminiCliConfig)
          : createApiExecutor(getGeminiApiClient())
    } else {
      throw new Error(`Unable to determine LLM provider for model: ${model}`)
    }

    executorCache.set(cacheKey, executor)
    return executor
  }
}

export const getExecutorForModel = createExecutorProvider()
