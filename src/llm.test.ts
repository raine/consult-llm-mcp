import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { EventEmitter } from 'events'
import type { Config } from './config.js'
import type { SupportedChatModel } from './schema.js'
import {
  getExecutorForModel,
  parseCodexJsonl,
  parseGeminiJson,
  parseCursorJson,
} from './llm.js'

const createCompletionMock = vi.hoisted(() => vi.fn())
const spawnMock = vi.hoisted(() => vi.fn())
const logCliDebugMock = vi.hoisted(() => vi.fn())

const mockConfig = vi.hoisted(
  () =>
    ({
      openaiApiKey: 'openai',
      geminiApiKey: 'gemini',
      deepseekApiKey: 'deepseek',
      openaiBackend: 'api',
      geminiBackend: 'api',
      defaultModel: undefined,
      codexReasoningEffort: undefined,
    }) as Config,
)

vi.mock('./config.js', () => ({ config: mockConfig }))
vi.mock('./logger.js', () => ({
  logCliDebug: logCliDebugMock,
  logToFile: vi.fn(),
}))
vi.mock('child_process', () => ({ spawn: spawnMock }))
vi.mock('openai', () => {
  class MockOpenAI {
    chat = {
      completions: {
        create: createCompletionMock,
      },
    }

    constructor(options: { apiKey: string; baseURL?: string }) {
      // store options if needed for assertions in the future
      void options
    }
  }

  return { default: MockOpenAI }
})

type FakeChildProcess = EventEmitter & {
  stdout: EventEmitter
  stderr: EventEmitter
  kill: ReturnType<typeof vi.fn>
}

const createChildProcess = (): FakeChildProcess => {
  const child = new EventEmitter() as FakeChildProcess
  child.stdout = new EventEmitter()
  child.stderr = new EventEmitter()
  child.kill = vi.fn()
  return child
}

const resolveCliExecution = (
  child: FakeChildProcess,
  {
    stdout = '',
    stderr = '',
    code = 0,
  }: { stdout?: string; stderr?: string; code?: number } = {},
) => {
  if (stdout) child.stdout.emit('data', stdout)
  if (stderr) child.stderr.emit('data', stderr)
  child.emit('close', code)
}

beforeEach(() => {
  createCompletionMock.mockReset()
  spawnMock.mockReset()
  logCliDebugMock.mockReset()
  Object.assign(mockConfig, {
    openaiApiKey: 'openai',
    geminiApiKey: 'gemini',
    deepseekApiKey: 'deepseek',
    openaiBackend: 'api',
    geminiBackend: 'api',
    defaultModel: undefined,
    codexReasoningEffort: undefined,
  })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('API executor', () => {
  it('sends system and user prompts and ignores file paths', async () => {
    const usage = { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 }
    createCompletionMock.mockResolvedValue({
      choices: [{ message: { content: 'answer' } }],
      usage,
    })
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const executor = getExecutorForModel('gpt-5.2')
    expect(executor.capabilities.isCli).toBe(false)
    expect(executor.capabilities.supportsThreads).toBe(false)
    expect(executor.capabilities.supportsFileRefs).toBe(false)

    const result = await executor.execute(
      'user prompt',
      'gpt-5.2',
      'system prompt',
      ['/tmp/file.ts'],
    )

    expect(createCompletionMock).toHaveBeenCalledWith({
      model: 'gpt-5.2',
      messages: [
        { role: 'system', content: 'system prompt' },
        { role: 'user', content: 'user prompt' },
      ],
    })
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('File paths were provided'),
    )
    expect(result).toEqual({ response: 'answer', usage })
  })

  it('throws when the API returns no content', async () => {
    createCompletionMock.mockResolvedValue({
      choices: [{ message: {} }],
    })

    const executor = getExecutorForModel('gpt-5.2')
    await expect(
      executor.execute('prompt', 'gpt-5.2', 'system'),
    ).rejects.toThrow('No response from the model via API')
  })
})

const codexJsonlOutput = (threadId: string, text: string) =>
  [
    JSON.stringify({ type: 'thread.started', thread_id: threadId }),
    JSON.stringify({
      type: 'item.completed',
      item: { type: 'agent_message', text },
    }),
  ].join('\n')

describe('parseCodexJsonl', () => {
  it('extracts thread_id and agent_message text', () => {
    const output = codexJsonlOutput('thread_abc', 'hello world')
    const result = parseCodexJsonl(output)
    expect(result.threadId).toBe('thread_abc')
    expect(result.response).toBe('hello world')
  })

  it('concatenates multiple agent_message items', () => {
    const output = [
      JSON.stringify({ type: 'thread.started', thread_id: 't1' }),
      JSON.stringify({
        type: 'item.completed',
        item: { type: 'agent_message', text: 'first' },
      }),
      JSON.stringify({
        type: 'item.completed',
        item: { type: 'agent_message', text: 'second' },
      }),
    ].join('\n')
    const result = parseCodexJsonl(output)
    expect(result.response).toBe('first\nsecond')
  })

  it('skips reasoning items', () => {
    const output = [
      JSON.stringify({ type: 'thread.started', thread_id: 't1' }),
      JSON.stringify({
        type: 'item.completed',
        item: { type: 'reasoning', text: 'thinking...' },
      }),
      JSON.stringify({
        type: 'item.completed',
        item: { type: 'agent_message', text: 'answer' },
      }),
    ].join('\n')
    const result = parseCodexJsonl(output)
    expect(result.response).toBe('answer')
  })

  it('skips non-JSON lines', () => {
    const output = [
      'ERROR: some log line',
      JSON.stringify({ type: 'thread.started', thread_id: 't1' }),
      'another garbage line',
      JSON.stringify({
        type: 'item.completed',
        item: { type: 'agent_message', text: 'result' },
      }),
    ].join('\n')
    const result = parseCodexJsonl(output)
    expect(result.threadId).toBe('t1')
    expect(result.response).toBe('result')
  })

  it('returns empty response when no agent_message found', () => {
    const output = JSON.stringify({ type: 'thread.started', thread_id: 't1' })
    const result = parseCodexJsonl(output)
    expect(result.threadId).toBe('t1')
    expect(result.response).toBe('')
  })
})

describe('Codex CLI executor', () => {
  const setupSpawn = (child: FakeChildProcess) => {
    spawnMock.mockReturnValue(child)
  }

  it('spawns codex CLI with --json and parses JSONL output', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    expect(executor.capabilities.isCli).toBe(true)
    expect(executor.capabilities.supportsThreads).toBe(true)

    const promise = executor.execute('user', 'gpt-5.2', 'system', [
      '/absolute/path/to/file.ts',
    ])

    resolveCliExecution(child, {
      stdout: codexJsonlOutput('thread_123', 'result'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    expect(args?.[0]).toBe('codex')
    const cliArgs = args?.[1] as string[]
    expect(cliArgs[0]).toBe('exec')
    expect(cliArgs[1]).toBe('--json')
    expect(cliArgs[2]).toBe('--skip-git-repo-check')
    expect(cliArgs).toContain('-m')
    expect(cliArgs).toContain('gpt-5.2')
    // Last arg is the prompt with system + user + files
    const promptArg = cliArgs[cliArgs.length - 1]
    expect(promptArg).toContain('system')
    expect(promptArg).toContain('user')
    expect(promptArg).toContain('Files: @')

    const result = await promise
    expect(result.response).toBe('result')
    expect(result.usage).toBeNull()
    expect(result.threadId).toBe('thread_123')
  })

  it('resumes a session with thread_id', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute(
      'follow up question',
      'gpt-5.2',
      'system',
      undefined,
      'thread_abc',
    )

    resolveCliExecution(child, {
      stdout: codexJsonlOutput('thread_abc', 'follow up answer'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    const cliArgs = args?.[1] as string[]
    expect(cliArgs[0]).toBe('exec')
    expect(cliArgs[1]).toBe('resume')
    expect(cliArgs[2]).toBe('--json')
    expect(cliArgs[3]).toBe('--skip-git-repo-check')
    expect(cliArgs).toContain('thread_abc')
    // Prompt should NOT contain system prompt on resume
    const promptArg = cliArgs[cliArgs.length - 1]
    expect(promptArg).toBe('follow up question')
    expect(promptArg).not.toContain('system')

    const result = await promise
    expect(result.response).toBe('follow up answer')
    expect(result.threadId).toBe('thread_abc')
  })

  it('rejects when no agent_message in JSONL output', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    resolveCliExecution(child, {
      stdout: JSON.stringify({ type: 'thread.started', thread_id: 't1' }),
      code: 0,
    })

    await expect(promise).rejects.toThrow(
      'No agent_message found in Codex JSONL output',
    )
  })

  it('rejects with codex errors on non-zero exit', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    resolveCliExecution(child, { stderr: 'boom', code: 2 })

    await expect(promise).rejects.toThrow(
      'Codex CLI exited with code 2. Error: boom',
    )
  })

  it('includes reasoning effort config when set', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    mockConfig.codexReasoningEffort = 'xhigh'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    resolveCliExecution(child, {
      stdout: codexJsonlOutput('t1', 'result'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    const cliArgs = args?.[1] as string[]
    expect(cliArgs).toContain('-c')
    expect(cliArgs).toContain('model_reasoning_effort="xhigh"')

    await promise
  })

  it('handles spawn error events with friendly message', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    child.emit('error', new Error('not found'))

    await expect(promise).rejects.toThrow(
      'Failed to spawn codex CLI. Is it installed and in PATH? Error: not found',
    )
  })

  it('handles synchronous spawn failures', async () => {
    mockConfig.openaiBackend = 'codex-cli'
    spawnMock.mockImplementation(() => {
      throw new Error('sync failure')
    })

    const executor = getExecutorForModel('gpt-5.2')
    await expect(executor.execute('user', 'gpt-5.2', 'system')).rejects.toThrow(
      'Synchronous error while trying to spawn codex: sync failure',
    )
  })
})

const geminiJsonOutput = (sessionId: string, response: string) =>
  JSON.stringify({ session_id: sessionId, response, stats: {} })

describe('parseGeminiJson', () => {
  it('extracts session_id and response', () => {
    const output = geminiJsonOutput('sess_abc', 'hello world')
    const result = parseGeminiJson(output)
    expect(result.sessionId).toBe('sess_abc')
    expect(result.response).toBe('hello world')
  })

  it('returns empty response when response is missing', () => {
    const output = JSON.stringify({ session_id: 's1' })
    const result = parseGeminiJson(output)
    expect(result.sessionId).toBe('s1')
    expect(result.response).toBe('')
  })
})

describe('Gemini CLI executor', () => {
  const setupSpawn = (child: FakeChildProcess) => {
    spawnMock.mockReturnValue(child)
  }

  it('spawns gemini CLI with -o json and parses JSON output', async () => {
    mockConfig.geminiBackend = 'gemini-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    expect(executor.capabilities.isCli).toBe(true)

    const promise = executor.execute('user prompt', 'gemini-2.5-pro', 'system')

    resolveCliExecution(child, {
      stdout: geminiJsonOutput('sess_123', 'result'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    expect(args?.[0]).toBe('gemini')
    const cliArgs = args?.[1] as string[]
    expect(cliArgs).toContain('-m')
    expect(cliArgs).toContain('gemini-2.5-pro')
    expect(cliArgs).toContain('-o')
    expect(cliArgs).toContain('json')
    expect(cliArgs).toContain('-p')

    const result = await promise
    expect(result.response).toBe('result')
    expect(result.usage).toBeNull()
    expect(result.threadId).toBe('sess_123')
  })

  it('resumes a session with thread_id', async () => {
    mockConfig.geminiBackend = 'gemini-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    const promise = executor.execute(
      'follow up',
      'gemini-2.5-pro',
      'system',
      undefined,
      'sess_abc',
    )

    resolveCliExecution(child, {
      stdout: geminiJsonOutput('sess_abc', 'follow up answer'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    const cliArgs = args?.[1] as string[]
    expect(cliArgs).toContain('-r')
    expect(cliArgs).toContain('sess_abc')
    // Prompt should NOT contain system prompt on resume
    const pIdx = cliArgs.indexOf('-p')
    expect(cliArgs[pIdx + 1]).toBe('follow up')

    const result = await promise
    expect(result.response).toBe('follow up answer')
    expect(result.threadId).toBe('sess_abc')
  })

  it('rejects when no response in JSON output', async () => {
    mockConfig.geminiBackend = 'gemini-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    const promise = executor.execute('user', 'gemini-2.5-pro', 'system')

    resolveCliExecution(child, {
      stdout: JSON.stringify({ session_id: 's1' }),
      code: 0,
    })

    await expect(promise).rejects.toThrow(
      'No response found in Gemini JSON output',
    )
  })

  it('rejects with parse error on invalid JSON', async () => {
    mockConfig.geminiBackend = 'gemini-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    const promise = executor.execute('user', 'gemini-2.5-pro', 'system')

    resolveCliExecution(child, { stdout: 'not json', code: 0 })

    await expect(promise).rejects.toThrow('Failed to parse Gemini JSON output')
  })

  it('wraps gemini quota errors specially', async () => {
    mockConfig.geminiBackend = 'gemini-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    const promise = executor.execute('user', 'gemini-2.5-pro', 'system')

    resolveCliExecution(child, {
      stderr: 'RESOURCE_EXHAUSTED: quota exceeded',
      code: 1,
    })

    await expect(promise).rejects.toThrow('Gemini quota exceeded')
  })

  it('handles spawn error events with friendly message', async () => {
    mockConfig.geminiBackend = 'gemini-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    const promise = executor.execute('user', 'gemini-2.5-pro', 'system')

    child.emit('error', new Error('not found'))

    await expect(promise).rejects.toThrow(
      'Failed to spawn gemini CLI. Is it installed and in PATH? Error: not found',
    )
  })
})

describe('parseCursorJson', () => {
  it('extracts session_id and result', () => {
    const output = JSON.stringify({
      session_id: 'sess_abc',
      result: 'hello world',
    })
    const result = parseCursorJson(output)
    expect(result.sessionId).toBe('sess_abc')
    expect(result.response).toBe('hello world')
  })

  it('returns empty response when result is missing', () => {
    const output = JSON.stringify({ session_id: 's1' })
    const result = parseCursorJson(output)
    expect(result.sessionId).toBe('s1')
    expect(result.response).toBe('')
  })
})

describe('Cursor CLI executor', () => {
  const setupSpawn = (child: FakeChildProcess) => {
    spawnMock.mockReturnValue(child)
  }

  const agentJsonOutput = (sessionId: string, result: string) =>
    JSON.stringify({ session_id: sessionId, result })

  it('spawns agent CLI and parses JSON output', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    expect(executor.capabilities.isCli).toBe(true)
    expect(executor.capabilities.supportsThreads).toBe(true)
    expect(executor.capabilities.supportsFileRefs).toBe(true)

    const promise = executor.execute('user prompt', 'gpt-5.2', 'system')

    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_123', 'result'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    expect(args?.[0]).toBe('cursor-agent')
    const cliArgs = args?.[1] as string[]
    expect(cliArgs).toContain('--print')
    expect(cliArgs).toContain('--trust')
    expect(cliArgs).toContain('--output-format')
    expect(cliArgs).toContain('json')
    expect(cliArgs).toContain('--mode')
    expect(cliArgs).toContain('ask')
    expect(cliArgs).toContain('--model')
    expect(cliArgs).toContain('gpt-5.2')

    const result = await promise
    expect(result.response).toBe('result')
    expect(result.usage).toBeNull()
    expect(result.threadId).toBe('sess_123')
  })

  it('resumes a session with thread_id', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute(
      'follow up',
      'gpt-5.2',
      'system',
      undefined,
      'sess_abc',
    )

    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_abc', 'follow up answer'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    const cliArgs = args?.[1] as string[]
    expect(cliArgs).toContain('--resume')
    expect(cliArgs).toContain('sess_abc')
    // Prompt should NOT contain system prompt on resume
    const promptArg = cliArgs[cliArgs.length - 1]
    expect(promptArg).toBe('follow up')
    expect(promptArg).not.toContain('system')

    const result = await promise
    expect(result.response).toBe('follow up answer')
    expect(result.threadId).toBe('sess_abc')
  })

  it('strips -preview from model names', async () => {
    mockConfig.geminiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-3-pro-preview')
    const promise = executor.execute('user', 'gemini-3-pro-preview', 'system')

    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_1', 'result'),
      code: 0,
    })

    const cliArgs = spawnMock.mock.calls[0]?.[1] as string[]
    expect(cliArgs).toContain('gemini-3-pro')
    expect(cliArgs).not.toContain('gemini-3-pro-preview')

    await promise
  })

  it('appends reasoning effort to codex model names', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    mockConfig.codexReasoningEffort = 'high'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.3-codex')
    const promise = executor.execute('user', 'gpt-5.3-codex', 'system')

    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_1', 'result'),
      code: 0,
    })

    const cliArgs = spawnMock.mock.calls[0]?.[1] as string[]
    expect(cliArgs).toContain('gpt-5.3-codex-high')

    await promise
  })

  it('does not append reasoning effort to non-codex models', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    mockConfig.codexReasoningEffort = 'high'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_1', 'result'),
      code: 0,
    })

    const cliArgs = spawnMock.mock.calls[0]?.[1] as string[]
    expect(cliArgs).toContain('gpt-5.2')
    expect(cliArgs).not.toContain('gpt-5.2-high')

    await promise
  })

  it('includes file refs when resuming a session', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute(
      'follow up',
      'gpt-5.2',
      'system',
      ['src/file.ts'],
      'sess_abc',
    )

    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_abc', 'answer'),
      code: 0,
    })

    const cliArgs = spawnMock.mock.calls[0]?.[1] as string[]
    const promptArg = cliArgs[cliArgs.length - 1]
    expect(promptArg).toContain('follow up')
    expect(promptArg).toContain('src/file.ts')
    expect(promptArg).not.toContain('system')

    await promise
  })

  it('routes GPT models to agent when openaiBackend is cursor-cli', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    expect(executor.capabilities.isCli).toBe(true)

    const promise = executor.execute('user', 'gpt-5.2', 'system')
    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_1', 'result'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    expect(args?.[0]).toBe('cursor-agent')

    const result = await promise
    expect(result.response).toBe('result')
  })

  it('routes Gemini models to agent when geminiBackend is cursor-cli', async () => {
    mockConfig.geminiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gemini-2.5-pro')
    expect(executor.capabilities.isCli).toBe(true)

    const promise = executor.execute('user', 'gemini-2.5-pro', 'system')
    resolveCliExecution(child, {
      stdout: agentJsonOutput('sess_1', 'result'),
      code: 0,
    })

    const args = spawnMock.mock.calls[0]
    expect(args?.[0]).toBe('cursor-agent')

    const result = await promise
    expect(result.response).toBe('result')
  })

  it('rejects when no result in JSON output', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    resolveCliExecution(child, {
      stdout: JSON.stringify({ session_id: 's1' }),
      code: 0,
    })

    await expect(promise).rejects.toThrow(
      'No result found in Cursor CLI JSON output',
    )
  })

  it('handles spawn error events with friendly message', async () => {
    mockConfig.openaiBackend = 'cursor-cli'
    const child = createChildProcess()
    setupSpawn(child)

    const executor = getExecutorForModel('gpt-5.2')
    const promise = executor.execute('user', 'gpt-5.2', 'system')

    child.emit('error', new Error('not found'))

    await expect(promise).rejects.toThrow(
      'Failed to spawn cursor-agent CLI. Is it installed and in PATH? Error: not found',
    )
  })
})

describe('executor selection', () => {
  it('uses deepseek API client', async () => {
    createCompletionMock.mockResolvedValue({
      choices: [{ message: { content: 'deepseek' } }],
    })
    const executor = getExecutorForModel('deepseek-reasoner')
    const result = await executor.execute(
      'prompt',
      'deepseek-reasoner',
      'system',
    )
    expect(result.response).toBe('deepseek')
  })

  it('caches and reuses executor instances', () => {
    const exec1 = getExecutorForModel('gpt-5.2')
    const exec2 = getExecutorForModel('gpt-5.2')
    expect(exec1).toBe(exec2)
  })

  it('creates distinct executors for different backends', () => {
    mockConfig.openaiBackend = 'api'
    const execApi = getExecutorForModel('gpt-5.2')
    mockConfig.openaiBackend = 'cursor-cli'
    const execCli = getExecutorForModel('gpt-5.2')
    expect(execApi).not.toBe(execCli)
  })

  it('throws on unknown models', () => {
    expect(() =>
      getExecutorForModel('mystery-model' as unknown as SupportedChatModel),
    ).toThrow('Unable to determine LLM provider')
  })
})
