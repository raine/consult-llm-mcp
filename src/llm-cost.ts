import { CompletionUsage } from 'openai/resources.js'
import { SupportedChatModel } from './llm.js'

// Model pricing data
type ModelPricing = {
  inputCostPerMillion: number
  outputCostPerMillion: number
}

const MODEL_PRICING: Partial<Record<SupportedChatModel, ModelPricing>> = {
  o3: {
    inputCostPerMillion: 2.0,
    outputCostPerMillion: 8.0,
  },
  'gemini-2.5-pro': {
    inputCostPerMillion: 1.25,
    outputCostPerMillion: 10.0,
  },
  'deepseek-reasoner': {
    inputCostPerMillion: 0.55,
    outputCostPerMillion: 2.19,
  },
}

export function calculateCost(
  usage: CompletionUsage | undefined,
  model: SupportedChatModel,
): { inputCost: number; outputCost: number; totalCost: number } {
  const pricing = MODEL_PRICING[model]
  if (!pricing) {
    return { inputCost: 0, outputCost: 0, totalCost: 0 }
  }

  const inputTokens = usage?.prompt_tokens || 0
  const outputTokens = usage?.completion_tokens || 0
  const inputCost = (inputTokens / 1_000_000) * pricing.inputCostPerMillion
  const outputCost = (outputTokens / 1_000_000) * pricing.outputCostPerMillion
  const totalCost = inputCost + outputCost

  return { inputCost, outputCost, totalCost }
}
