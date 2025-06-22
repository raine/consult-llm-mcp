#!/usr/bin/env node

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { getClientForModel, SupportedChatModel } from './llm.js'
import { readFileSync, existsSync, appendFileSync, mkdirSync } from 'fs'
import { resolve, join } from 'path'
import { homedir } from 'os'

// Setup logging directory
const logDir = join(homedir(), '.llmtool', 'logs')
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
    name: 'llmtool',
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
    tools: [
      {
        name: 'llm_query',
        description:
          'Ask a more powerful AI for help with complex problems. Write your problem description in a markdown file and pass relevant code files as context.',
        inputSchema: {
          type: 'object',
          properties: {
            files: {
              type: 'array',
              items: { type: 'string' },
              description:
                'Array of file paths to process. Markdown files (.md) become the main prompt, other files are added as context with file paths and code blocks.',
            },
            model: {
              type: 'string',
              enum: ['o3', 'gemini-2.5-pro'],
              default: 'o3',
              description: 'LLM model to use',
            },
          },
          required: ['files'],
        },
      },
    ],
  }
})

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  if (request.params.name === 'llm_query') {
    const { files, model = 'o3' } = request.params.arguments as {
      files: string[]
      model?: SupportedChatModel
    }

    try {
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

        if (
          originalPath.endsWith('.md') ||
          originalPath.endsWith('.markdown')
        ) {
          markdownFiles.push(content)
        } else {
          otherFiles.push({ path: originalPath, content })
        }
      }

      // Build prompt using same logic as CLI
      let promptParts: string[] = []

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

      // Log the response
      logToFile(`RESPONSE (model: ${model}):\n${response}\n${'='.repeat(80)}`)

      return {
        content: [
          {
            type: 'text',
            text: response,
          },
        ],
      }
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
