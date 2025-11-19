import { describe, it, expect } from 'vitest'
import { ConsultLlmArgs, SupportedChatModel } from './schema.js'

describe('SupportedChatModel', () => {
  it('accepts known models and rejects unsupported ones', () => {
    expect(SupportedChatModel.safeParse('o3').success).toBe(true)
    expect(SupportedChatModel.safeParse('gpt-3.5').success).toBe(false)
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

  it('defaults model to o3 when omitted', () => {
    const parsed = ConsultLlmArgs.parse({ prompt: 'hello world' })
    expect(parsed.model).toBe('o3')
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
