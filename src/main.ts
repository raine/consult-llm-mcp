import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { getClientForModel, SupportedChatModel } from './llm.js'
import { config } from './config.js'
import { readFileSync, existsSync, appendFileSync, mkdirSync } from 'fs'
import { resolve, join } from 'path'
import { homedir } from 'os'
import { execSync } from 'child_process'
import { CompletionUsage } from 'openai/resources.js'

// Model pricing data
type ModelPricing = {
  inputCostPerMillion: number
  outputCostPerMillion: number
}

const MODEL_PRICING: Partial<Record<SupportedChatModel, ModelPricing>> = {
  o3: {
    inputCostPerMillion: 2.0,
    outputCostPerMillion: 8.0,
  },
  'gemini-2.5-pro': {
    inputCostPerMillion: 1.25,
    outputCostPerMillion: 10.0,
  },
  'deepseek-reasoner': {
    inputCostPerMillion: 0.55,
    outputCostPerMillion: 2.19,
  },
}

function calculateCost(
  usage: CompletionUsage | undefined,
  model: SupportedChatModel,
): { inputCost: number; outputCost: number; totalCost: number } {
  const pricing = MODEL_PRICING[model]
  if (!pricing) {
    return { inputCost: 0, outputCost: 0, totalCost: 0 }
  }

  const inputTokens = usage?.prompt_tokens || 0
  const outputTokens = usage?.completion_tokens || 0
  const inputCost = (inputTokens / 1_000_000) * pricing.inputCostPerMillion
  const outputCost = (outputTokens / 1_000_000) * pricing.outputCostPerMillion
  const totalCost = inputCost + outputCost

  return { inputCost, outputCost, totalCost }
}

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
    tools: [
      {
        name: 'consult_llm',
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
              enum: ['o3', 'gemini-2.5-pro', 'deepseek-reasoner'],
              default: 'o3',
              description: 'LLM model to use',
            },
            git_diff: {
              type: 'object',
              properties: {
                repo_path: {
                  type: 'string',
                  description:
                    'Path to git repository (defaults to current working directory)',
                },
                files: {
                  type: 'array',
                  items: { type: 'string' },
                  description: 'Specific files to include in diff',
                },
                base_ref: {
                  type: 'string',
                  default: 'HEAD',
                  description:
                    'Git reference to compare against (e.g., "HEAD", "main", commit hash)',
                },
              },
              required: ['files'],
              description:
                'Generate git diff output to include as context. Shows uncommitted changes by default.',
            },
          },
          required: ['files'],
        },
      },
    ],
  }
})

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  if (request.params.name === 'consult_llm') {
    // Log the tool call with all parameters
    logToFile(
      `TOOL CALL: ${request.params.name}\nArguments: ${JSON.stringify(request.params.arguments, null, 2)}\n${'='.repeat(80)}`,
    )

    const {
      files,
      model = config.defaultModel || 'o3',
      git_diff,
    } = request.params.arguments as {
      files: string[]
      model?: SupportedChatModel
      git_diff?: {
        repo_path?: string
        files: string[]
        base_ref?: string
      }
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
