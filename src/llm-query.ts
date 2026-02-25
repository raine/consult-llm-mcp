import type { LlmExecutor } from './executors/types.js'
import type { SupportedChatModel, TaskMode } from './schema.js'
import { calculateCost } from './llm-cost.js'
import { getSystemPrompt } from './system-prompt.js'

export async function queryLlm(
  prompt: string,
  model: SupportedChatModel,
  executor: LlmExecutor,
  filePaths?: string[],
  threadId?: string,
  taskMode?: TaskMode,
): Promise<{
  response: string
  costInfo: string
  threadId?: string
}> {
  const systemPrompt = getSystemPrompt(executor.capabilities.isCli, taskMode)

  const {
    response,
    usage,
    threadId: returnedThreadId,
  } = await executor.execute(prompt, model, systemPrompt, filePaths, threadId)

  if (!response) {
    throw new Error('No response from the model')
  }

  let costInfo: string
  if (usage) {
    const { inputCost, outputCost, totalCost } = calculateCost(usage, model)
    costInfo = `Tokens: ${usage.prompt_tokens} input, ${usage.completion_tokens} output | Cost: $${totalCost.toFixed(6)} (input: $${inputCost.toFixed(6)}, output: $${outputCost.toFixed(6)})`
  } else {
    costInfo = 'Cost data not available (using CLI mode)'
  }

  return { response, costInfo, threadId: returnedThreadId }
}
