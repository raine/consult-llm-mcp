import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, readFileSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import { join, resolve } from 'path'
import type { Config } from './config.js'
import type { SupportedChatModel } from './schema.js'
import { handleConsultLlm, initSystemPrompt } from './server.js'

const processFilesMock = vi.hoisted(() => vi.fn())
const generateGitDiffMock = vi.hoisted(() => vi.fn())
const buildPromptMock = vi.hoisted(() => vi.fn())
const queryLlmMock = vi.hoisted(() => vi.fn())
const getSystemPromptMock = vi.hoisted(() => vi.fn())
const copyToClipboardMock = vi.hoisted(() => vi.fn())
const logToolCallMock = vi.hoisted(() => vi.fn())
const logPromptMock = vi.hoisted(() => vi.fn())
const logResponseMock = vi.hoisted(() => vi.fn())
const logServerStartMock = vi.hoisted(() => vi.fn())
const logConfigurationMock = vi.hoisted(() => vi.fn())

const apiCapabilities = {
  isCli: false,
  supportsThreads: false,
  supportsFileRefs: false,
}
const cliCapabilities = {
  isCli: true,
  supportsThreads: true,
  supportsFileRefs: true,
}

const mockGetExecutorForModel = vi.hoisted(() => vi.fn())

const mockConfig = vi.hoisted(
  () =>
    ({
      openaiBackend: 'api',
      geminiBackend: 'api',
      defaultModel: undefined,
    }) as Config,
)

vi.mock('./config.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./config.js')>()
  return {
    ...actual,
    config: mockConfig,
  }
})
vi.mock('./file.js', () => ({ processFiles: processFilesMock }))
vi.mock('./git.js', () => ({ generateGitDiff: generateGitDiffMock }))
vi.mock('./prompt-builder.js', () => ({ buildPrompt: buildPromptMock }))
vi.mock('./llm-query.js', () => ({ queryLlm: queryLlmMock }))
vi.mock('./system-prompt.js', () => ({
  DEFAULT_SYSTEM_PROMPT: '# default prompt',
  getSystemPrompt: getSystemPromptMock,
}))
vi.mock('./clipboard.js', () => ({ copyToClipboard: copyToClipboardMock }))
vi.mock('./logger.js', () => ({
  logToolCall: logToolCallMock,
  logPrompt: logPromptMock,
  logResponse: logResponseMock,
  logServerStart: logServerStartMock,
  logConfiguration: logConfigurationMock,
}))
vi.mock('./llm.js', () => ({
  getExecutorForModel: mockGetExecutorForModel,
}))

beforeEach(() => {
  processFilesMock.mockReset().mockReturnValue([{ path: 'a.ts', content: '' }])
  generateGitDiffMock.mockReset().mockReturnValue('diff output')
  buildPromptMock.mockReset().mockReturnValue('BUILT PROMPT')
  queryLlmMock.mockReset().mockResolvedValue({
    response: 'ok',
    costInfo: null,
  })
  getSystemPromptMock.mockReset().mockReturnValue('SYSTEM PROMPT')
  copyToClipboardMock.mockReset().mockResolvedValue(undefined)
  logToolCallMock.mockReset()
  logPromptMock.mockReset()
  logResponseMock.mockReset()
  mockGetExecutorForModel.mockReset()
  Object.assign(mockConfig, {
    openaiBackend: 'api',
    geminiBackend: 'api',
    defaultModel: undefined,
  })

  // Default: return API executor
  mockGetExecutorForModel.mockReturnValue({
    capabilities: apiCapabilities,
    execute: vi.fn(),
  })
})

describe('handleConsultLlm', () => {
  it('validates input', async () => {
    await expect(handleConsultLlm({})).rejects.toThrow(
      'Invalid request parameters',
    )
  })

  it('inlines files and git diff for API mode', async () => {
    mockConfig.defaultModel = 'gpt-5.2' as SupportedChatModel
    const result = await handleConsultLlm({
      prompt: 'help me',
      files: ['file1.ts'],
      git_diff: { files: ['src/index.ts'] },
    })

    expect(processFilesMock).toHaveBeenCalledWith(['file1.ts'])
    expect(generateGitDiffMock).toHaveBeenCalledWith(
      undefined,
      ['src/index.ts'],
      'HEAD',
    )
    expect(buildPromptMock).toHaveBeenCalledWith(
      'help me',
      expect.any(Array),
      'diff output',
    )
    expect(queryLlmMock).toHaveBeenCalledWith(
      'BUILT PROMPT',
      'gpt-5.2',
      expect.objectContaining({ capabilities: apiCapabilities }),
      undefined,
      undefined,
      'general',
    )
    expect(result.content[0]?.text).toBe('ok')
  })

  it('uses explicit model even when config default exists', async () => {
    mockConfig.defaultModel = 'gpt-5.2' as SupportedChatModel
    await handleConsultLlm({ prompt: 'hello', model: 'gpt-5.2' })
    expect(queryLlmMock).toHaveBeenCalledWith(
      'BUILT PROMPT',
      'gpt-5.2',
      expect.objectContaining({ capabilities: apiCapabilities }),
      undefined,
      undefined,
      'general',
    )
  })

  it('builds CLI prompts without file contents', async () => {
    mockGetExecutorForModel.mockReturnValue({
      capabilities: cliCapabilities,
      execute: vi.fn(),
    })

    await handleConsultLlm({
      prompt: 'cli prompt',
      files: ['./foo.ts'],
      git_diff: { files: ['foo.ts'], base_ref: 'main', repo_path: '/repo' },
    })

    expect(processFilesMock).not.toHaveBeenCalled()
    expect(buildPromptMock).not.toHaveBeenCalled()
    const [prompt, model, , filePaths] = queryLlmMock.mock.calls[0] as [
      string,
      SupportedChatModel,
      unknown,
      string[] | undefined,
    ]
    expect(prompt).toMatchInlineSnapshot(`
      "## Git Diff
      \`\`\`diff
      diff output
      \`\`\`

      cli prompt"
    `)
    expect(model).toBe('gpt-5.2')
    expect(filePaths).toEqual([resolve('./foo.ts')])
  })

  it('handles web mode by copying to clipboard and skipping LLM call', async () => {
    const result = await handleConsultLlm({
      prompt: 'web prompt',
      files: ['file.ts'],
      web_mode: true,
    })

    expect(copyToClipboardMock).toHaveBeenCalled()
    const [copied] = copyToClipboardMock.mock.calls[0] as [string]
    expect(copied).toMatchInlineSnapshot(`
      "# System Prompt

      SYSTEM PROMPT

      # User Prompt

      BUILT PROMPT"
    `)
    expect(queryLlmMock).not.toHaveBeenCalled()
    expect(result.content[0]?.text).toContain('Prompt copied to clipboard')
  })

  it('passes thread_id to queryLlm for CLI backend models', async () => {
    mockGetExecutorForModel.mockReturnValue({
      capabilities: cliCapabilities,
      execute: vi.fn(),
    })

    await handleConsultLlm({
      prompt: 'follow up',
      model: 'gpt-5.2',
      thread_id: 'thread_abc',
    })

    const callArgs = queryLlmMock.mock.calls[0] as unknown[]
    expect(callArgs[4]).toBe('thread_abc')
  })

  it('prefixes response with thread_id when returned', async () => {
    mockGetExecutorForModel.mockReturnValue({
      capabilities: cliCapabilities,
      execute: vi.fn(),
    })
    queryLlmMock.mockResolvedValueOnce({
      response: 'answer',
      costInfo: null,
      threadId: 'thread_xyz',
    })

    const result = await handleConsultLlm({
      prompt: 'question',
      model: 'gpt-5.2',
    })

    expect(result.content[0]?.text).toBe('[thread_id:thread_xyz]\n\nanswer')
  })

  it('passes thread_id to queryLlm for Gemini CLI backend', async () => {
    mockGetExecutorForModel.mockReturnValue({
      capabilities: cliCapabilities,
      execute: vi.fn(),
    })

    await handleConsultLlm({
      prompt: 'follow up',
      model: 'gemini-2.5-pro',
      thread_id: 'sess_abc',
    })

    const callArgs = queryLlmMock.mock.calls[0] as unknown[]
    expect(callArgs[4]).toBe('sess_abc')
  })

  it('rejects thread_id with non-CLI backend model', async () => {
    mockGetExecutorForModel.mockReturnValue({
      capabilities: apiCapabilities,
      execute: vi.fn(),
    })

    await expect(
      handleConsultLlm({
        prompt: 'hello',
        model: 'gpt-5.2',
        thread_id: 'thread_abc',
      }),
    ).rejects.toThrow('thread_id is not supported by the configured backend')
  })

  it('rejects thread_id with Gemini API backend', async () => {
    mockGetExecutorForModel.mockReturnValue({
      capabilities: apiCapabilities,
      execute: vi.fn(),
    })

    await expect(
      handleConsultLlm({
        prompt: 'hello',
        model: 'gemini-2.5-pro',
        thread_id: 'sess_abc',
      }),
    ).rejects.toThrow('thread_id is not supported by the configured backend')
  })

  it('passes task_mode to queryLlm', async () => {
    await handleConsultLlm({ prompt: 'plan this', task_mode: 'plan' })
    const callArgs = queryLlmMock.mock.calls[0] as unknown[]
    expect(callArgs[5]).toBe('plan')
  })

  it('passes task_mode to getSystemPrompt in web mode', async () => {
    await handleConsultLlm({
      prompt: 'create content',
      web_mode: true,
      task_mode: 'create',
    })
    expect(getSystemPromptMock).toHaveBeenCalledWith(false, 'create')
  })

  it('propagates query errors', async () => {
    queryLlmMock.mockRejectedValueOnce(new Error('boom'))
    await expect(handleConsultLlm({ prompt: 'oops' })).rejects.toThrow('boom')
  })
})

describe('initSystemPrompt', () => {
  let tempHome: string

  beforeEach(() => {
    tempHome = mkdtempSync(join(tmpdir(), 'consult-llm-home-'))
  })

  afterEach(() => {
    rmSync(tempHome, { recursive: true, force: true })
  })

  const stubExit = () =>
    vi.spyOn(process, 'exit').mockImplementation(() => undefined as never)

  it('creates a default system prompt file', () => {
    const exitSpy = stubExit()
    initSystemPrompt(tempHome)
    const promptPath = join(tempHome, '.consult-llm-mcp', 'SYSTEM_PROMPT.md')
    const contents = readFileSync(promptPath, 'utf-8')
    expect(contents).toBe('# default prompt')
    expect(exitSpy).toHaveBeenCalledWith(0)
    exitSpy.mockRestore()
  })

  it('rejects reinitialization when file exists', () => {
    const exitSpy = stubExit()
    initSystemPrompt(tempHome)
    exitSpy.mockClear()

    initSystemPrompt(tempHome)
    expect(exitSpy).toHaveBeenCalledWith(1)
    exitSpy.mockRestore()
  })
})
