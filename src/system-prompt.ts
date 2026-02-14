import { readFileSync, existsSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import { config } from './config.js'
import { type TaskMode } from './schema.js'

const BASE_SYSTEM_PROMPT = `You are an expert software engineering consultant. You are communicating with another AI system, not a human.

Communication style:
- Skip pleasantries and praise
- Be direct and specific
- Respond in Markdown`

const MODE_OVERLAYS: Record<TaskMode, string> = {
  review: `Your role is to:
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

Be critical and thorough. Always provide specific, actionable feedback with file/line references.`,

  plan: `Your role is to:
- Explore multiple approaches and evaluate trade-offs
- Consider scalability, maintainability, and simplicity
- Provide concrete recommendations with rationale
- Think about edge cases and failure modes
- Suggest incremental implementation strategies

Be constructive and thorough. Present options clearly with pros and cons.`,

  create: `Your role is to:
- Generate clear, well-structured content
- Match the appropriate tone and level of detail for the audience
- Provide complete, ready-to-use output
- Include relevant examples where helpful
- Focus on clarity and correctness

Be helpful and thorough. Produce polished, high-quality output.`,

  general: '',
}

/**
 * The full default system prompt (base + review overlay) for backward
 * compatibility and for the `init-prompt` CLI command.
 */
export const DEFAULT_SYSTEM_PROMPT = `${BASE_SYSTEM_PROMPT}\n\n${MODE_OVERLAYS.review}`

const CLI_MODE_SUFFIX = `

IMPORTANT: Do not edit files yourself, only provide recommendations and code examples`

export function getSystemPrompt(
  isCliMode: boolean,
  taskMode: TaskMode = 'review',
): string {
  const customPromptPath =
    config.systemPromptPath ??
    join(homedir(), '.consult-llm-mcp', 'SYSTEM_PROMPT.md')

  if (existsSync(customPromptPath)) {
    try {
      const customPrompt = readFileSync(customPromptPath, 'utf-8').trim()
      // Custom prompt is a full override — no mode overlays applied
      return isCliMode ? customPrompt + CLI_MODE_SUFFIX : customPrompt
    } catch (error) {
      console.error(
        `Warning: Failed to read custom system prompt from ${customPromptPath}:`,
        error,
      )
    }
  }

  const overlay = MODE_OVERLAYS[taskMode]
  const systemPrompt = overlay
    ? `${BASE_SYSTEM_PROMPT}\n\n${overlay}`
    : BASE_SYSTEM_PROMPT

  return isCliMode ? systemPrompt + CLI_MODE_SUFFIX : systemPrompt
}
