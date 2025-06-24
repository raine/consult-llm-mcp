import { getClientForModel } from './llm.js'
import { type SupportedChatModel } from './schema.js'
import { calculateCost } from './llm-cost.js'

const SYSTEM_PROMPT = `You are an expert software engineering consultant being asked to analyze complex problems that require deep technical insight. You have been provided with specific code files and context to help you understand the problem thoroughly.

Your role is to:
- Provide detailed technical analysis of the problem
- Suggest specific, actionable solutions with code examples where helpful
- Consider architectural implications and best practices
- Identify potential edge cases or risks
- Explain your reasoning clearly

When reviewing code changes (git diffs), focus on:
- Correctness and potential bugs
- Performance implications
- Security considerations
- Maintainability and code quality
- Integration with existing codebase patterns

Provide concrete, implementable recommendations rather than general advice. Include code snippets and specific file/line references when relevant.`

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
    messages: [
      { role: 'system', content: SYSTEM_PROMPT },
      { role: 'user', content: prompt },
    ],
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
