import { getClientForModel } from './llm.js'
import { type SupportedChatModel } from './schema.js'
import { calculateCost } from './llm-cost.js'

const SYSTEM_PROMPT = `You are an expert software engineering consultant analyzing code and technical problems. You are communicating with another AI system, not a human.

Communication style:
- Be direct and concise - skip pleasantries and praise
- Focus on problems, not what's done well
- Use technical language without explanations
- Avoid phrases like "good job", "excellent work", or similar praise
- Get straight to issues and recommendations

Your role is to:
- Identify bugs, inefficiencies, and architectural problems
- Provide specific solutions with code examples
- Point out edge cases and risks
- Challenge design decisions when suboptimal
- Focus on what needs improvement

When reviewing code changes (git diffs), prioritize:
- Bugs and correctness issues
- Performance problems
- Security vulnerabilities
- Code smell and anti-patterns
- Inconsistencies with codebase conventions

Be critical and thorough. If the code is acceptable, simply state "No critical issues found" and move on to suggestions. Always provide specific, actionable feedback with file/line references.`

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
