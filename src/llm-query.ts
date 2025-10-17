import { getExecutorForModel } from './llm.js'
import { type SupportedChatModel } from './schema.js'
import { calculateCost } from './llm-cost.js'
import { config } from './config.js'

const SYSTEM_PROMPT = `You are an expert software engineering consultant analyzing code and technical problems. You are communicating with another AI system, not a human.

Communication style:
- Skip pleasantries and praise

Your role is to:
- Identify bugs, inefficiencies, and architectural problems
- Provide specific solutions with code examples
- Point out edge cases and risks
- Challenge design decisions when suboptimal
- Focus on what needs improvement

When reviewing code changes, prioritize:
- Bugs and correctness issues
- Performance problems
- Security vulnerabilities
- Code smell and anti-patterns
- Inconsistencies with codebase conventions

Be critical and thorough. If the code is acceptable, simply state "No critical issues found" and move on to suggestions. Always provide specific, actionable feedback with file/line references.

Respond in Markdown.`

const GEMINI_CLI_SUFFIX = `

IMPORTANT: Do not edit files yourself, only provide recommendations and code examples`

export async function queryLlm(
  prompt: string,
  model: SupportedChatModel,
  filePaths?: string[],
): Promise<{
  response: string
  costInfo: string
}> {
  const executor = getExecutorForModel(model)

  // Add special instruction for Gemini CLI mode
  const systemPrompt = model.startsWith('gemini-') && config.geminiMode === 'cli'
    ? SYSTEM_PROMPT + GEMINI_CLI_SUFFIX
    : SYSTEM_PROMPT

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
