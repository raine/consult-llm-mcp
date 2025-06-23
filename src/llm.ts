import OpenAI from 'openai'
import { config } from './config.js'
import { z } from 'zod/v4'
import {
  SupportedChatModel,
  type SupportedChatModel as SupportedChatModelType,
} from './schema.js'

const clients: { openai?: OpenAI; gemini?: OpenAI; deepseek?: OpenAI } = {}

export function getClientForModel(model: SupportedChatModelType): {
  client: OpenAI
} {
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
    return { client: clients.openai }
  } else if (model.startsWith('gemini-')) {
    if (!clients.gemini) {
      if (!config.geminiApiKey) {
        throw new Error(
          'GEMINI_API_KEY environment variable is required for Gemini models',
        )
      }
      clients.gemini = new OpenAI({
        apiKey: config.geminiApiKey,
        baseURL: 'https://generativelanguage.googleapis.com/v1beta/openai/',
      })
    }
    return { client: clients.gemini }
  } else if (model.startsWith('deepseek-')) {
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
    return { client: clients.deepseek }
  } else {
    throw new Error(`Unable to determine LLM provider for model: ${model}`)
  }
}
