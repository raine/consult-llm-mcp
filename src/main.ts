#!/usr/bin/env node

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { getClientForModel } from './llm.js'
import { config } from './config.js'
import { calculateCost } from './llm-cost.js'
import { ConsultLlmArgs, SupportedChatModel, toolSchema } from './schema.js'
import { readFileSync, existsSync, appendFileSync, mkdirSync } from 'fs'
import { resolve, join } from 'path'
import { homedir } from 'os'
import { execSync } from 'child_process'
import { z } from 'zod/v4'

// Setup logging directory
const logDir = join(homedir(), '.consult-llm-mcp', 'logs')
const logFile = join(logDir, 'mcp.log')

try {
  mkdirSync(logDir, { recursive: true })
} catch (error) {
  // Directory might already exist
}

function logToFile(content: string) {
  const timestamp = new Date().toISOString()
  const logEntry = `[${timestamp}] ${content}\n`
  try {
    appendFileSync(logFile, logEntry)
  } catch (error) {
    console.error('Failed to write to log file:', error)
  }
}

const server = new Server(
  {
    name: 'consult_llm',
    version: '1.0.0',
  },
  {
    capabilities: {
      tools: {},
    },
  },
)

server.setRequestHandler(ListToolsRequestSchema, async () => {
  return {
    tools: [toolSchema],
  }
})

async function handleConsultLlm(args: unknown) {
  // Log the tool call with all parameters
  logToFile(
    `TOOL CALL: consult_llm\nArguments: ${JSON.stringify(args, null, 2)}\n${'='.repeat(80)}`,
  )

  const parseResult = ConsultLlmArgs.safeParse(args)

  if (!parseResult.success) {
    const errors = parseResult.error.issues
      .map((issue) => `${issue.path.join('.')}: ${issue.message}`)
      .join(', ')
    throw new Error(`Invalid request parameters: ${errors}`)
  }

  const { files, git_diff } = parseResult.data
  const model: SupportedChatModel =
    parseResult.data.model ?? config.defaultModel ?? 'o3'

  // Validate files exist
  const resolvedFiles = files.map((f) => resolve(f))
  const missingFiles = resolvedFiles.filter((f) => !existsSync(f))
  if (missingFiles.length > 0) {
    throw new Error(`Files not found: ${missingFiles.join(', ')}`)
  }

  // Process files using same logic as CLI
  const markdownFiles: string[] = []
  const otherFiles: { path: string; content: string }[] = []

  for (let i = 0; i < files.length; i++) {
    const filePath = resolvedFiles[i]
    const originalPath = files[i]
    const content = readFileSync(filePath, 'utf-8')

    if (originalPath.endsWith('.md') || originalPath.endsWith('.markdown')) {
      markdownFiles.push(content)
    } else {
      otherFiles.push({ path: originalPath, content })
    }
  }

  // Generate git diff if requested
  let gitDiffOutput = ''
  if (git_diff) {
    try {
      const repoPath = git_diff.repo_path || process.cwd()
      const diffFiles = git_diff.files
      const baseRef = git_diff.base_ref || 'HEAD'

      // Build git diff command - always pass specific files to avoid unrelated changes
      if (diffFiles.length === 0) {
        throw new Error('No files specified for git diff')
      }
      const gitCommand = `git diff ${baseRef} -- ${diffFiles.join(' ')}`

      gitDiffOutput = execSync(gitCommand, {
        cwd: repoPath,
        encoding: 'utf-8',
        maxBuffer: 1024 * 1024, // 1MB max
      })
    } catch (error) {
      gitDiffOutput = `Error generating git diff: ${error instanceof Error ? error.message : String(error)}`
    }
  }

  // Build prompt using same logic as CLI
  let promptParts: string[] = []

  // Add git diff as context if available
  if (gitDiffOutput.trim()) {
    promptParts.push('## Git Diff\n')
    promptParts.push('```diff')
    promptParts.push(gitDiffOutput)
    promptParts.push('```\n')
  }

  // Add non-markdown files as context
  if (otherFiles.length > 0) {
    promptParts.push('## Relevant Files\n')
    for (const file of otherFiles) {
      promptParts.push(`### File: ${file.path}`)
      promptParts.push('```')
      promptParts.push(file.content)
      promptParts.push('```\n')
    }
  }

  // Add markdown files as main prompt
  if (markdownFiles.length > 0) {
    promptParts.push(...markdownFiles)
  }

  const prompt = promptParts.join('\n')

  // Log the prompt
  logToFile(`PROMPT (model: ${model}):\n${prompt}\n${'='.repeat(80)}`)

  // Send to LLM
  const { client } = getClientForModel(model)
  const completion = await client.chat.completions.create({
    model,
    messages: [{ role: 'user', content: prompt }],
  })

  const response = completion.choices[0]?.message?.content
  if (!response) {
    throw new Error('No response from the model')
  }

  // Calculate and log pricing
  const usage = completion.usage
  const { inputCost, outputCost, totalCost } = calculateCost(usage, model)
  const costInfo = usage
    ? `Tokens: ${usage.prompt_tokens} input, ${usage.completion_tokens} output | Cost: $${totalCost.toFixed(6)} (input: $${inputCost.toFixed(6)}, output: $${outputCost.toFixed(6)})`
    : 'Usage data not available'

  // Log the response with pricing
  logToFile(
    `RESPONSE (model: ${model}):\n${response}\n${costInfo}\n${'='.repeat(80)}`,
  )

  return {
    content: [
      {
        type: 'text',
        text: response,
      },
    ],
  }
}

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  if (request.params.name === 'consult_llm') {
    try {
      return await handleConsultLlm(request.params.arguments)
    } catch (error) {
      throw new Error(
        `LLM query failed: ${error instanceof Error ? error.message : String(error)}`,
      )
    }
  }

  throw new Error(`Unknown tool: ${request.params.name}`)
})

async function main() {
  const transport = new StdioServerTransport()
  await server.connect(transport)
}

main().catch((error) => {
  console.error('Fatal error:', error)
  process.exit(1)
})
