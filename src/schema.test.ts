import { describe, it, expect } from 'vitest'
import { ConsultLlmArgs, SupportedChatModel, ALL_MODELS } from './schema.js'

describe('SupportedChatModel', () => {
  it('accepts known models and rejects unsupported ones', () => {
    expect(SupportedChatModel.safeParse('gpt-5.2').success).toBe(true)
    expect(SupportedChatModel.safeParse('gpt-5.1-codex-max').success).toBe(true)
    expect(SupportedChatModel.safeParse('gpt-3.5').success).toBe(false)
  })

  it('ALL_MODELS contains all available models', () => {
    expect(ALL_MODELS).toContain('gpt-5.2')
    expect(ALL_MODELS).toContain('gemini-2.5-pro')
    expect(ALL_MODELS).toContain('gemini-3-pro-preview')
    expect(ALL_MODELS.length).toBeGreaterThan(0)
  })
})

describe('ConsultLlmArgs', () => {
  it('requires prompt', () => {
    const result = ConsultLlmArgs.safeParse({})
    expect(result.success).toBe(false)
  })

  it('enforces non-empty git diff files', () => {
    const result = ConsultLlmArgs.safeParse({
      prompt: 'hey',
      git_diff: { files: [] },
    })
    expect(result.success).toBe(false)
    if (!result.success) {
      expect(result.error.issues[0]?.message).toContain('At least one file')
    }
  })

  it('applies default base_ref value', () => {
    const result = ConsultLlmArgs.safeParse({
      prompt: 'test',
      git_diff: { files: ['a.ts'] },
    })
    expect(result.success).toBe(true)
    if (result.success) {
      expect(result.data.git_diff?.base_ref).toBe('HEAD')
    }
  })

  it('defaults model to a valid enabled model when omitted', () => {
    const parsed = ConsultLlmArgs.parse({ prompt: 'hello world' })
    expect(parsed.model).toBeDefined()
    expect(ALL_MODELS).toContain(parsed.model)
  })

  it('defaults web_mode to false but honors explicit value', () => {
    const parsedDefault = ConsultLlmArgs.parse({ prompt: 'default case' })
    expect(parsedDefault.web_mode).toBe(false)

    const parsedTrue = ConsultLlmArgs.parse({
      prompt: 'explicit',
      web_mode: true,
    })
    expect(parsedTrue.web_mode).toBe(true)
  })
})
