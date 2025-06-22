import { getClientForModel } from './llm.js'
import { type SupportedChatModel } from './schema.js'
import { calculateCost } from './llm-cost.js'

export async function queryLlm(
  prompt: string,
  model: SupportedChatModel,
): Promise<{
  response: string
  costInfo: string
}> {
  const { client } = getClientForModel(model)
  const completion = await client.chat.completions.create({
    model,
    messages: [{ role: 'user', content: prompt }],
  })

  const response = completion.choices[0]?.message?.content
  if (!response) {
    throw new Error('No response from the model')
  }

  const usage = completion.usage

  // Calculate costs
  const { inputCost, outputCost, totalCost } = calculateCost(usage, model)
  const costInfo = usage
    ? `Tokens: ${usage.prompt_tokens} input, ${usage.completion_tokens} output | Cost: $${totalCost.toFixed(6)} (input: $${inputCost.toFixed(6)}, output: $${outputCost.toFixed(6)})`
    : 'Usage data not available'

  return { response, costInfo }
}
