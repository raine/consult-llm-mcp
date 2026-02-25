import { describe, it, expect, vi } from 'vitest'
import { migrateBackendEnv, buildModelCatalog } from './config.js'
import { ALL_MODELS } from './models.js'

vi.mock('./logger.js', () => ({ logToFile: vi.fn() }))

describe('migrateBackendEnv', () => {
  it('returns newVar when set, ignoring oldVar', () => {
    expect(
      migrateBackendEnv(
        'cursor-cli',
        'cli',
        'gemini-cli',
        'GEMINI_MODE',
        'GEMINI_BACKEND',
      ),
    ).toBe('cursor-cli')
  })

  it('maps "cli" to provider-specific cli value', () => {
    expect(
      migrateBackendEnv(
        undefined,
        'cli',
        'gemini-cli',
        'GEMINI_MODE',
        'GEMINI_BACKEND',
      ),
    ).toBe('gemini-cli')
  })

  it('passes through non-cli values directly', () => {
    expect(
      migrateBackendEnv(
        undefined,
        'api',
        'gemini-cli',
        'GEMINI_MODE',
        'GEMINI_BACKEND',
      ),
    ).toBe('api')
  })

  it('returns undefined when both vars are missing', () => {
    expect(
      migrateBackendEnv(
        undefined,
        undefined,
        'gemini-cli',
        'GEMINI_MODE',
        'GEMINI_BACKEND',
      ),
    ).toBeUndefined()
  })

  it('maps openai cli to codex-cli', () => {
    expect(
      migrateBackendEnv(
        undefined,
        'cli',
        'codex-cli',
        'OPENAI_MODE',
        'OPENAI_BACKEND',
      ),
    ).toBe('codex-cli')
  })
})

describe('buildModelCatalog', () => {
  it('returns all built-in models when no env vars are set', () => {
    const result = buildModelCatalog(ALL_MODELS)
    expect(result).toEqual([...ALL_MODELS])
  })

  it('appends extra models to the catalog', () => {
    const result = buildModelCatalog(ALL_MODELS, 'grok-3,kimi-k2.5')
    expect(result).toContain('grok-3')
    expect(result).toContain('kimi-k2.5')
    expect(result.length).toBe(ALL_MODELS.length + 2)
  })

  it('deduplicates extra models that overlap with built-ins', () => {
    const result = buildModelCatalog(ALL_MODELS, 'gpt-5.2,grok-3')
    expect(result.filter((m) => m === 'gpt-5.2').length).toBe(1)
    expect(result.length).toBe(ALL_MODELS.length + 1)
  })

  it('filters by allowlist from combined catalog', () => {
    const result = buildModelCatalog(ALL_MODELS, 'grok-3', 'gpt-5.2,grok-3')
    expect(result).toEqual(['gpt-5.2', 'grok-3'])
  })

  it('allowlist can include only extra models', () => {
    const result = buildModelCatalog(ALL_MODELS, 'grok-3', 'grok-3')
    expect(result).toEqual(['grok-3'])
  })

  it('allowlist filters out models not in catalog', () => {
    const result = buildModelCatalog(ALL_MODELS, undefined, 'nonexistent')
    expect(result).toEqual([])
  })

  it('handles whitespace and empty entries in extra models', () => {
    const result = buildModelCatalog(ALL_MODELS, ' grok-3 , , kimi-k2.5 ')
    expect(result).toContain('grok-3')
    expect(result).toContain('kimi-k2.5')
    expect(result.length).toBe(ALL_MODELS.length + 2)
  })

  it('handles whitespace in allowlist', () => {
    const result = buildModelCatalog(
      ALL_MODELS,
      undefined,
      ' gpt-5.2 , gemini-2.5-pro ',
    )
    expect(result).toContain('gpt-5.2')
    expect(result).toContain('gemini-2.5-pro')
    expect(result.length).toBe(2)
  })

  it('preserves built-in model order with extras appended', () => {
    const result = buildModelCatalog(ALL_MODELS, 'aaa-model,zzz-model')
    const builtinPart = result.slice(0, ALL_MODELS.length)
    expect(builtinPart).toEqual([...ALL_MODELS])
    expect(result.slice(ALL_MODELS.length)).toEqual(['aaa-model', 'zzz-model'])
  })
})
