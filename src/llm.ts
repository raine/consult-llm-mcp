import OpenAI from 'openai'
import { spawn } from 'child_process'
import { relative } from 'path'
import { config } from './config.js'
import { type SupportedChatModel as SupportedChatModelType } from './schema.js'
import { logCliDebug } from './logger.js'

// --- Executor Interface Definition ---
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

// --- API Executor Implementation ---
class ApiExecutor implements LlmExecutor {
  private client: OpenAI

  constructor(client: OpenAI) {
    this.client = client
  }

  async execute(
    prompt: string,
    model: SupportedChatModelType,
    systemPrompt: string,
  ): Promise<{ response: string; usage: OpenAI.CompletionUsage | null }> {
    const completion = await this.client.chat.completions.create({
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
  }
}

// --- Base CLI Executor Implementation ---
abstract class BaseCliExecutor implements LlmExecutor {
  protected abstract getCliName(): string
  protected abstract buildArgs(
    model: SupportedChatModelType,
    fullPrompt: string,
  ): string[]
  protected abstract handleNonZeroExit(code: number, stderr: string): Error

  protected buildFullPrompt(
    prompt: string,
    systemPrompt: string,
    filePaths?: string[],
  ): string {
    let fullPrompt = `${systemPrompt}\n\n${prompt}`

    // Append file references using @ syntax
    if (filePaths && filePaths.length > 0) {
      const fileReferences = filePaths
        .map((path) => `@${relative(process.cwd(), path)}`)
        .join(' ')
      fullPrompt = `${fullPrompt}\n\nFiles: ${fileReferences}`
    }

    return fullPrompt
  }

  async execute(
    prompt: string,
    model: SupportedChatModelType,
    systemPrompt: string,
    filePaths?: string[],
  ): Promise<{ response: string; usage: OpenAI.CompletionUsage | null }> {
    const fullPrompt = this.buildFullPrompt(prompt, systemPrompt, filePaths)
    const args = this.buildArgs(model, fullPrompt)
    const cliName = this.getCliName()

    return new Promise((resolve, reject) => {
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

      child.on('spawn', () => {
        logCliDebug(`${cliName} CLI process spawned successfully`)
      })

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

      child.stdout.on('data', (data: Buffer) => {
        stdout += data.toString()
      })

      child.stderr.on('data', (data: Buffer) => {
        stderr += data.toString()
      })

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
          reject(this.handleNonZeroExit(code ?? -1, stderr))
        }
      })

      child.on('error', (err) => {
        hasResponded = true
        clearTimeout(timeout)
        logCliDebug(`Failed to spawn ${cliName} CLI`, { error: err.message })
        reject(
          new Error(
            `Failed to spawn ${cliName} CLI. Is it installed and in PATH? Error: ${err.message}`,
          ),
        )
      })
    })
  }
}

// --- Gemini CLI Executor Implementation ---
class GeminiCliExecutor extends BaseCliExecutor {
  protected getCliName(): string {
    return 'gemini'
  }

  protected buildArgs(
    model: SupportedChatModelType,
    fullPrompt: string,
  ): string[] {
    return ['-m', model, '-p', fullPrompt]
  }

  protected handleNonZeroExit(code: number, stderr: string): Error {
    if (stderr.includes('RESOURCE_EXHAUSTED')) {
      return new Error(
        `Gemini quota exceeded. Consider using gemini-2.0-flash model. Error: ${stderr.trim()}`,
      )
    }
    return new Error(
      `Gemini CLI exited with code ${code}. Error: ${stderr.trim()}`,
    )
  }
}

// --- Codex CLI Executor Implementation ---
class CodexCliExecutor extends BaseCliExecutor {
  protected getCliName(): string {
    return 'codex'
  }

  protected buildArgs(
    model: SupportedChatModelType,
    fullPrompt: string,
  ): string[] {
    // Per documentation: `codex -m <model> [PROMPT]`
    // The prompt is a positional argument, not behind a flag
    return ['-m', model, fullPrompt]
  }

  protected handleNonZeroExit(code: number, stderr: string): Error {
    // Generic error handling. Codex may have specific errors
    // to parse, like rate limits, which should be added here
    // after observing its behavior.
    return new Error(
      `Codex CLI exited with code ${code}. Error: ${stderr.trim()}`,
    )
  }
}

// --- Client Cache and Executor Factory ---
const clients: { openai?: OpenAI; geminiApi?: OpenAI; deepseek?: OpenAI } = {}
let geminiCliExecutor: GeminiCliExecutor | undefined
let codexCliExecutor: CodexCliExecutor | undefined

export function getExecutorForModel(
  model: SupportedChatModelType,
): LlmExecutor {
  if (model.startsWith('gpt-') || model === 'o3') {
    // Check for CLI mode for OpenAI models
    if (config.openaiMode === 'cli') {
      if (!codexCliExecutor) {
        codexCliExecutor = new CodexCliExecutor()
      }
      return codexCliExecutor
    }

    // Fallback to API mode
    if (!clients.openai) {
      if (!config.openaiApiKey) {
        throw new Error(
          'OPENAI_API_KEY environment variable is required for OpenAI models in API mode',
        )
      }
      clients.openai = new OpenAI({
        apiKey: config.openaiApiKey,
      })
    }
    return new ApiExecutor(clients.openai)
  }

  if (model.startsWith('deepseek-')) {
    if (!clients.deepseek) {
      if (!config.deepseekApiKey) {
        throw new Error(
          'DEEPSEEK_API_KEY environment variable is required for DeepSeek models',
        )
      }
      clients.deepseek = new OpenAI({
        apiKey: config.deepseekApiKey,
        baseURL: 'https://api.deepseek.com',
      })
    }
    return new ApiExecutor(clients.deepseek)
  }

  if (model.startsWith('gemini-')) {
    // Check if CLI mode is enabled
    if (config.geminiMode === 'cli') {
      if (!geminiCliExecutor) {
        geminiCliExecutor = new GeminiCliExecutor()
      }
      return geminiCliExecutor
    }

    if (!clients.geminiApi) {
      if (!config.geminiApiKey) {
        throw new Error(
          'GEMINI_API_KEY environment variable is required for Gemini models in API mode',
        )
      }
      clients.geminiApi = new OpenAI({
        apiKey: config.geminiApiKey,
        baseURL: 'https://generativelanguage.googleapis.com/v1beta/openai/',
      })
    }
    return new ApiExecutor(clients.geminiApi)
  }

  throw new Error(`Unable to determine LLM provider for model: ${model}`)
}
