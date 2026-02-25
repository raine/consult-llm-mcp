import { describe, it, expect, vi } from 'vitest'
import { migrateBackendEnv } from './config.js'

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
