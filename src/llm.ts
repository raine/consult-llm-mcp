import OpenAI from 'openai'
import { config } from './config.js'
import { z } from 'zod/v4'
import {
  SupportedChatModel,
  type SupportedChatModel as SupportedChatModelType,
} from './schema.js'

const clients: { openai?: OpenAI; gemini?: OpenAI; deepseek?: OpenAI } = {}

export function getClientForModel(model: SupportedChatModelType | string): {
  client: OpenAI
} {
  if (model.startsWith('gpt-') || model === 'o3') {
    if (!clients.openai) {
      clients.openai = new OpenAI({
        apiKey: config.openaiApiKey,
      })
    }
    return { client: clients.openai }
  } else if (model.startsWith('gemini-')) {
    if (!clients.gemini) {
      clients.gemini = new OpenAI({
        apiKey: config.geminiApiKey,
        baseURL: 'https://generativelanguage.googleapis.com/v1beta/openai/',
      })
    }
    return { client: clients.gemini }
  } else if (model.startsWith('deepseek-')) {
    if (!clients.deepseek) {
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
