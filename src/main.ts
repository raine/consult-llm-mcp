#!/usr/bin/env node

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { config } from './config.js'
import {
  ConsultLlmArgs,
  toolSchema,
  type SupportedChatModel,
} from './schema.js'
import { processFiles } from './file.js'
import { generateGitDiff } from './git.js'
import { buildPrompt } from './prompt-builder.js'
import { queryLlm } from './llm-query.js'
import {
  logToolCall,
  logPrompt,
  logResponse,
  logServerStart,
} from './logger.js'
import { readFileSync } from 'fs'
import { dirname, join } from 'path'
import { fileURLToPath } from 'url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const packageJson = JSON.parse(
  readFileSync(join(__dirname, '../package.json'), 'utf-8'),
)
const SERVER_VERSION = packageJson.version

const server = new Server(
  {
    name: 'consult_llm',
    version: SERVER_VERSION,
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

  logToolCall('consult_llm', args)

  // Process files
  const { markdownFiles, otherFiles } = processFiles(files)

  // Generate git diff
  const gitDiffOutput = git_diff
    ? generateGitDiff(git_diff.repo_path, git_diff.files, git_diff.base_ref)
    : undefined

  // Build prompt
  const prompt = buildPrompt(markdownFiles, otherFiles, gitDiffOutput)
  logPrompt(model, prompt)

  // Query LLM
  const { response, costInfo } = await queryLlm(prompt, model)
  logResponse(model, response, costInfo)

  return {
    content: [{ type: 'text', text: response }],
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
  logServerStart(SERVER_VERSION)

  const transport = new StdioServerTransport()
  await server.connect(transport)
}

main().catch((error) => {
  console.error('Fatal error:', error)
  process.exit(1)
})
