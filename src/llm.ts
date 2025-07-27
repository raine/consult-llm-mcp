import OpenAI from 'openai'
import { spawn } from 'child_process'
import { config } from './config.js'
import { z } from 'zod/v4'
import {
  SupportedChatModel,
  type SupportedChatModel as SupportedChatModelType,
} from './schema.js'

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
    filePaths?: string[],
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

// --- CLI Executor Implementation ---
class CliExecutor implements LlmExecutor {
  async execute(
    prompt: string,
    model: SupportedChatModelType,
    systemPrompt: string,
    filePaths?: string[],
  ): Promise<{ response: string; usage: OpenAI.CompletionUsage | null }> {
    // Build the full prompt with system prompt prepended
    let fullPrompt = `${systemPrompt}\n\n${prompt}`

    // Append file references using @ syntax
    if (filePaths && filePaths.length > 0) {
      // Escape spaces in paths for CLI
      const fileReferences = filePaths
        .map((path) => `@${path.replace(/ /g, '\\ ')}`)
        .join(' ')
      fullPrompt = `${fullPrompt}\n\nFiles: ${fileReferences}`
    }

    const args = ['-m', model, '-p', fullPrompt]

    return new Promise((resolve, reject) => {
      const child = spawn('gemini', args, { shell: false })
      let stdout = ''
      let stderr = ''

      child.stdout.on('data', (data) => (stdout += data.toString()))
      child.stderr.on('data', (data) => (stderr += data.toString()))

      child.on('close', (code) => {
        if (code === 0) {
          resolve({ response: stdout.trim(), usage: null })
        } else {
          // Check for quota exceeded error
          if (stderr.includes('RESOURCE_EXHAUSTED')) {
            reject(
              new Error(
                `Gemini quota exceeded. Consider using gemini-2.0-flash model. Error: ${stderr.trim()}`,
              ),
            )
          } else {
            reject(
              new Error(
                `Gemini CLI exited with code ${code}. Error: ${stderr.trim()}`,
              ),
            )
          }
        }
      })

      child.on('error', (err) => {
        reject(
          new Error(
            `Failed to spawn Gemini CLI. Is it installed and in PATH? Error: ${err.message}`,
          ),
        )
      })
    })
  }
}

// --- Client Cache and Executor Factory ---
const clients: { openai?: OpenAI; geminiApi?: OpenAI; deepseek?: OpenAI } = {}
let geminiCliExecutor: CliExecutor | undefined

export function getExecutorForModel(
  model: SupportedChatModelType,
): LlmExecutor {
  if (model.startsWith('gpt-') || model === 'o3') {
    if (!clients.openai) {
      if (!config.openaiApiKey) {
        throw new Error(
          'OPENAI_API_KEY environment variable is required for OpenAI models',
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
        geminiCliExecutor = new CliExecutor()
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
