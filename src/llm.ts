import OpenAI from 'openai'
import { config } from './config.js'
import type { SupportedChatModel as SupportedChatModelType } from './schema.js'
import type { LlmExecutor } from './executors/types.js'
import { createApiExecutor } from './executors/api.js'
import { createGeminiExecutor } from './executors/gemini-cli.js'
import { createCodexExecutor } from './executors/codex-cli.js'
import { createCursorExecutor } from './executors/cursor-cli.js'

// Re-export for consumers
export type { LlmExecutor, LlmExecutorCapabilities } from './executors/types.js'
export { parseGeminiJson } from './executors/gemini-cli.js'
export { parseCodexJsonl } from './executors/codex-cli.js'
export { parseCursorJson } from './executors/cursor-cli.js'

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
    const cacheKey =
      model +
      (model.startsWith('gpt-') ? `-${config.openaiBackend}` : '') +
      (model.startsWith('gemini-') ? `-${config.geminiBackend}` : '')

    if (executorCache.has(cacheKey)) {
      return executorCache.get(cacheKey)!
    }

    let executor: LlmExecutor

    if (model.startsWith('gpt-')) {
      if (config.openaiBackend === 'codex-cli') {
        executor = createCodexExecutor()
      } else if (config.openaiBackend === 'cursor-cli') {
        executor = createCursorExecutor()
      } else {
        executor = createApiExecutor(getOpenAIClient())
      }
    } else if (model.startsWith('deepseek-')) {
      executor = createApiExecutor(getDeepseekClient())
    } else if (model.startsWith('gemini-')) {
      if (config.geminiBackend === 'gemini-cli') {
        executor = createGeminiExecutor()
      } else if (config.geminiBackend === 'cursor-cli') {
        executor = createCursorExecutor()
      } else {
        executor = createApiExecutor(getGeminiApiClient())
      }
    } else {
      throw new Error(`Unable to determine LLM provider for model: ${model}`)
    }

    executorCache.set(cacheKey, executor)
    return executor
  }
}

export const getExecutorForModel = createExecutorProvider()
