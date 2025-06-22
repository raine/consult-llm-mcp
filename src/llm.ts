import OpenAI from 'openai'
import { config } from './config.js'

const clients: { openai?: OpenAI; gemini?: OpenAI } = {}

export type SupportedChatModel = 'o3' | 'gemini-2.5-pro'

export function getClientForModel(model: SupportedChatModel | string): {
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
  } else {
    throw new Error(`Unable to determine LLM provider for model: ${model}`)
  }
}
