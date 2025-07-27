import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'
import { format } from 'prettier'

const logDir = join(homedir(), '.consult-llm-mcp', 'logs')
const logFile = join(logDir, 'mcp.log')

try {
  mkdirSync(logDir, { recursive: true })
} catch (error) {
  // Directory might already exist
}

async function formatWithPrettier(text: string): Promise<string> {
  try {
    return await format(text, {
      parser: 'markdown',
      printWidth: 80,
      proseWrap: 'always',
    })
  } catch (error) {
    // If formatting fails, return original text
    return text
  }
}

export function logToFile(content: string) {
  const timestamp = new Date().toISOString()
  const logEntry = `[${timestamp}] ${content}\n`
  try {
    appendFileSync(logFile, logEntry)
  } catch (error) {
    console.error('Failed to write to log file:', error)
  }
}

export function logToolCall(name: string, args: unknown) {
  logToFile(
    `TOOL CALL: ${name}\nArguments: ${JSON.stringify(args, null, 2)}\n${'='.repeat(80)}`,
  )
}

export async function logPrompt(model: string, prompt: string) {
  const formattedPrompt = await formatWithPrettier(prompt)
  logToFile(`PROMPT (model: ${model}):\n${formattedPrompt}\n${'='.repeat(80)}`)
}

export async function logResponse(
  model: string,
  response: string,
  costInfo: string,
) {
  const formattedResponse = await formatWithPrettier(response)
  logToFile(
    `RESPONSE (model: ${model}):\n${formattedResponse}\n${costInfo}\n${'='.repeat(80)}`,
  )
}

export function logServerStart(version: string) {
  logToFile(
    `MCP SERVER STARTED - consult-llm-mcp v${version}\n${'='.repeat(80)}`,
  )
}

export function logConfiguration(config: Record<string, any>) {
  const redactedConfig = Object.entries(config).reduce(
    (acc, [key, value]) => {
      // Redact API keys and other sensitive values
      if (
        key.toLowerCase().includes('key') ||
        key.toLowerCase().includes('secret')
      ) {
        acc[key] = value ? '[REDACTED]' : undefined
      } else {
        acc[key] = value
      }
      return acc
    },
    {} as Record<string, any>,
  )

  logToFile(
    `CONFIGURATION:\n${JSON.stringify(redactedConfig, null, 2)}\n${'='.repeat(80)}`,
  )
}
