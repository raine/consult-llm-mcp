import { readFileSync, existsSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'

export const DEFAULT_SYSTEM_PROMPT = `You are an expert software engineering consultant analyzing code and technical problems. You are communicating with another AI system, not a human.

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

Be critical and thorough. Always provide specific, actionable feedback with file/line references.

Respond in Markdown.`

const CLI_MODE_SUFFIX = `

IMPORTANT: Do not edit files yourself, only provide recommendations and code examples`

export function getSystemPrompt(isCliMode: boolean): string {
  const customPromptPath = join(
    homedir(),
    '.consult-llm-mcp',
    'SYSTEM_PROMPT.md',
  )
  let systemPrompt: string

  if (existsSync(customPromptPath)) {
    try {
      systemPrompt = readFileSync(customPromptPath, 'utf-8').trim()
    } catch (error) {
      console.error(
        `Warning: Failed to read custom system prompt from ${customPromptPath}:`,
        error,
      )
      systemPrompt = DEFAULT_SYSTEM_PROMPT
    }
  } else {
    systemPrompt = DEFAULT_SYSTEM_PROMPT
  }

  return isCliMode ? systemPrompt + CLI_MODE_SUFFIX : systemPrompt
}
