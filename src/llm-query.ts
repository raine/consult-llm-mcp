import { getExecutorForModel } from './llm.js'
import { type SupportedChatModel } from './schema.js'
import { calculateCost } from './llm-cost.js'
import { config } from './config.js'
import { getSystemPrompt } from './system-prompt.js'

export async function queryLlm(
  prompt: string,
  model: SupportedChatModel,
  filePaths?: string[],
): Promise<{
  response: string
  costInfo: string
}> {
  const executor = getExecutorForModel(model)

  // Get system prompt (with Gemini CLI suffix if needed)
  const isGeminiCli = model.startsWith('gemini-') && config.geminiMode === 'cli'
  const systemPrompt = getSystemPrompt(isGeminiCli)

  const { response, usage } = await executor.execute(
    prompt,
    model,
    systemPrompt,
    filePaths,
  )

  if (!response) {
    throw new Error('No response from the model')
  }

  let costInfo: string
  if (usage) {
    // Calculate costs only if usage data is available (from API)
    const { inputCost, outputCost, totalCost } = calculateCost(usage, model)
    costInfo = `Tokens: ${usage.prompt_tokens} input, ${usage.completion_tokens} output | Cost: $${totalCost.toFixed(6)} (input: $${inputCost.toFixed(6)}, output: $${outputCost.toFixed(6)})`
  } else {
    // Handle case where usage is not available (from CLI)
    costInfo = 'Cost data not available (using CLI mode)'
  }

  return { response, costInfo }
}
