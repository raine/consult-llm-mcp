import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { homedir } from 'os'

const logDir = join(homedir(), '.consult-llm-mcp', 'logs')
const logFile = join(logDir, 'mcp.log')

try {
  mkdirSync(logDir, { recursive: true })
} catch (error) {
  // Directory might already exist
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

export function logPrompt(model: string, prompt: string) {
  logToFile(`PROMPT (model: ${model}):\n${prompt}\n${'='.repeat(80)}`)
}

export function logResponse(model: string, response: string, costInfo: string) {
  logToFile(
    `RESPONSE (model: ${model}):\n${response}\n${costInfo}\n${'='.repeat(80)}`,
  )
}

export function logServerStart(version: string) {
  logToFile(
    `MCP SERVER STARTED - consult-llm-mcp v${version}\n${'='.repeat(80)}`,
  )
}
