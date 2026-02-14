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
  logConfiguration,
} from './logger.js'
import { DEFAULT_SYSTEM_PROMPT, getSystemPrompt } from './system-prompt.js'
import { copyToClipboard } from './clipboard.js'
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'fs'
import { dirname, join, resolve } from 'path'
import { fileURLToPath } from 'url'
import { homedir } from 'os'

const __dirname = dirname(fileURLToPath(import.meta.url))
const packageJson = JSON.parse(
  readFileSync(join(__dirname, '../package.json'), 'utf-8'),
) as { version: string }
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

server.setRequestHandler(ListToolsRequestSchema, () => {
  return {
    tools: [toolSchema],
  }
})

export function isCliExecution(model: SupportedChatModel): boolean {
  if (model.startsWith('gemini-') && config.geminiMode === 'cli') {
    return true
  }
  if (model.startsWith('gpt-') && config.openaiMode === 'cli') {
    return true
  }
  return false
}

export async function handleConsultLlm(args: unknown) {
  const parseResult = ConsultLlmArgs.safeParse(args)
  if (!parseResult.success) {
    const errors = parseResult.error.issues
      .map((issue) => `${issue.path.join('.')}: ${issue.message}`)
      .join(', ')
    throw new Error(`Invalid request parameters: ${errors}`)
  }

  const {
    files,
    prompt: userPrompt,
    git_diff,
    web_mode,
    model: parsedModel,
    thread_id: threadId,
    task_mode: taskMode,
  } = parseResult.data

  const providedModel =
    typeof args === 'object' &&
    args !== null &&
    Object.prototype.hasOwnProperty.call(
      args as Record<string, unknown>,
      'model',
    )

  const model: SupportedChatModel = providedModel
    ? parsedModel
    : (config.defaultModel ?? parsedModel)

  logToolCall('consult_llm', args)

  const isCliMode = isCliExecution(model)

  if (threadId && !isCliMode) {
    throw new Error(
      'thread_id is only supported with CLI mode models (Codex or Gemini CLI)',
    )
  }

  let prompt: string
  let filePaths: string[] | undefined

  if (web_mode || !isCliMode) {
    const contextFiles = files ? processFiles(files) : []

    const gitDiffOutput = git_diff
      ? generateGitDiff(git_diff.repo_path, git_diff.files, git_diff.base_ref)
      : undefined

    prompt = buildPrompt(userPrompt, contextFiles, gitDiffOutput)
  } else {
    filePaths = files ? files.map((f) => resolve(f)) : undefined

    const gitDiffOutput = git_diff
      ? generateGitDiff(git_diff.repo_path, git_diff.files, git_diff.base_ref)
      : undefined

    prompt = gitDiffOutput
      ? `## Git Diff\n\`\`\`diff\n${gitDiffOutput}\n\`\`\`\n\n${userPrompt}`
      : userPrompt
  }

  await logPrompt(model, prompt)

  if (web_mode) {
    const systemPrompt = getSystemPrompt(isCliMode, taskMode)
    const fullPrompt = `# System Prompt

${systemPrompt}

# User Prompt

${prompt}`

    await copyToClipboard(fullPrompt)

    let responseMessage = '✓ Prompt copied to clipboard!\n\n'
    responseMessage +=
      'Please paste it into your browser-based LLM service and share the response here before I proceed with any implementation.'

    if (filePaths && filePaths.length > 0) {
      responseMessage += `\n\nNote: File paths were included:\n${filePaths.map((p) => `  - ${p}`).join('\n')}`
    }

    return {
      content: [{ type: 'text', text: responseMessage }],
    }
  }

  const {
    response,
    costInfo,
    threadId: returnedThreadId,
  } = await queryLlm(prompt, model, filePaths, threadId, taskMode)
  await logResponse(model, response, costInfo)

  const responseText = returnedThreadId
    ? `[thread_id:${returnedThreadId}]\n\n${response}`
    : response

  return {
    content: [{ type: 'text', text: responseText }],
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

export function initSystemPrompt(homeDirectory: string = homedir()) {
  const configDir = join(homeDirectory, '.consult-llm-mcp')
  const promptPath = join(configDir, 'SYSTEM_PROMPT.md')

  if (existsSync(promptPath)) {
    console.error(`System prompt already exists at: ${promptPath}`)
    console.error('Remove it first if you want to reinitialize.')
    process.exit(1)
  }

  if (!existsSync(configDir)) {
    mkdirSync(configDir, { recursive: true })
  }

  writeFileSync(promptPath, DEFAULT_SYSTEM_PROMPT, 'utf-8')
  console.log(`Created system prompt at: ${promptPath}`)
  console.log('You can now edit this file to customize the system prompt.')
  process.exit(0)
}

export async function main() {
  if (process.argv.includes('--version') || process.argv.includes('-v')) {
    console.log(SERVER_VERSION)
    process.exit(0)
  }

  if (process.argv.includes('init-prompt')) {
    initSystemPrompt()
    return
  }

  logServerStart(SERVER_VERSION)
  logConfiguration(config)

  const transport = new StdioServerTransport()
  await server.connect(transport)
}
