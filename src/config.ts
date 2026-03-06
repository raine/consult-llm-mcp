import { z } from 'zod/v4'
import { ALL_MODELS } from './models.js'
import { logToFile } from './logger.js'

export interface ProviderAvailability {
  geminiApiKey?: string
  geminiBackend: string
  openaiApiKey?: string
  openaiBackend: string
  deepseekApiKey?: string
}

export function filterByAvailability(
  models: string[],
  providers: ProviderAvailability,
): string[] {
  return models.filter((model) => {
    if (model.startsWith('gemini-')) {
      return providers.geminiBackend !== 'api' || !!providers.geminiApiKey
    }
    if (model.startsWith('gpt-')) {
      return providers.openaiBackend !== 'api' || !!providers.openaiApiKey
    }
    if (model.startsWith('deepseek-')) {
      return !!providers.deepseekApiKey
    }
    // Unknown prefix (user-added extra models) — always include
    return true
  })
}

/** Build the final model catalog from built-in + extra + allowlist filtering. */
export function buildModelCatalog(
  builtinModels: readonly string[],
  extraModelsRaw?: string,
  allowedModelsRaw?: string,
): string[] {
  const extraModels = extraModelsRaw
    ? extraModelsRaw
        .split(',')
        .map((m) => m.trim())
        .filter((m) => m.length > 0)
    : []

  const allAvailable: string[] = [
    ...builtinModels,
    ...extraModels.filter((m) => !builtinModels.includes(m)),
  ]

  const allowedModels = allowedModelsRaw
    ? allowedModelsRaw
        .split(',')
        .map((m) => m.trim())
        .filter((m) => m.length > 0)
    : []

  return allowedModels.length > 0
    ? allAvailable.filter((m) => allowedModels.includes(m))
    : allAvailable
}

// Resolve backends early (needed for availability filtering)
// Priority: CONSULT_LLM_*_BACKEND > *_BACKEND > *_MODE (deprecated)
const geminiBackendRaw = migratePrefixedEnv(
  process.env.CONSULT_LLM_GEMINI_BACKEND,
  process.env.GEMINI_BACKEND,
  'GEMINI_BACKEND',
  'CONSULT_LLM_GEMINI_BACKEND',
)
const resolvedGeminiBackend = migrateBackendEnv(
  geminiBackendRaw,
  process.env.GEMINI_MODE,
  'gemini-cli',
  'GEMINI_MODE',
  'CONSULT_LLM_GEMINI_BACKEND',
)

const openaiBackendRaw = migratePrefixedEnv(
  process.env.CONSULT_LLM_OPENAI_BACKEND,
  process.env.OPENAI_BACKEND,
  'OPENAI_BACKEND',
  'CONSULT_LLM_OPENAI_BACKEND',
)
const resolvedOpenaiBackend = migrateBackendEnv(
  openaiBackendRaw,
  process.env.OPENAI_MODE,
  'codex-cli',
  'OPENAI_MODE',
  'CONSULT_LLM_OPENAI_BACKEND',
)

// Build catalog, then filter to only available providers
const catalogModels = buildModelCatalog(
  ALL_MODELS,
  process.env.CONSULT_LLM_EXTRA_MODELS,
  process.env.CONSULT_LLM_ALLOWED_MODELS,
)

const enabledModels = filterByAvailability(catalogModels, {
  geminiApiKey: process.env.GEMINI_API_KEY,
  geminiBackend: resolvedGeminiBackend ?? 'api',
  openaiApiKey: process.env.OPENAI_API_KEY,
  openaiBackend: resolvedOpenaiBackend ?? 'api',
  deepseekApiKey: process.env.DEEPSEEK_API_KEY,
})

if (enabledModels.length === 0) {
  const msg =
    'Invalid environment variables:\n  No models available. Set API keys or configure CLI backends.'
  logToFile(`FATAL ERROR:\n${msg}`)
  console.error(`❌ ${msg}`)
  process.exit(1)
}

// Dynamic Zod enum based on enabled models
export const SupportedChatModel = z.enum(enabledModels as [string, ...string[]])
export type SupportedChatModel = z.infer<typeof SupportedChatModel>

export const fallbackModel = enabledModels.includes('gpt-5.2')
  ? 'gpt-5.2'
  : enabledModels[0]

const Config = z.object({
  openaiApiKey: z.string().optional(),
  geminiApiKey: z.string().optional(),
  deepseekApiKey: z.string().optional(),
  defaultModel: SupportedChatModel.optional(),
  geminiBackend: z.enum(['api', 'gemini-cli', 'cursor-cli']).default('api'),
  openaiBackend: z.enum(['api', 'codex-cli', 'cursor-cli']).default('api'),
  codexReasoningEffort: z
    .enum(['none', 'minimal', 'low', 'medium', 'high', 'xhigh'])
    .optional(),
  systemPromptPath: z.string().optional(),
})

type ParsedConfig = z.infer<typeof Config>

export type Config = ParsedConfig & {
  allowedModels: string[]
}

/** Prefer CONSULT_LLM_-prefixed env var, fall back to unprefixed with deprecation warning. */
export function migratePrefixedEnv(
  prefixed: string | undefined,
  unprefixed: string | undefined,
  unprefixedName: string,
  prefixedName: string,
): string | undefined {
  if (prefixed) return prefixed
  if (!unprefixed) return undefined
  logToFile(
    `DEPRECATED: ${unprefixedName}=${unprefixed} → use ${prefixedName}=${unprefixed} instead`,
  )
  return unprefixed
}

// Migrate legacy GEMINI_MODE / OPENAI_MODE env vars
export function migrateBackendEnv(
  newVar: string | undefined,
  oldVar: string | undefined,
  providerCliValue: string,
  legacyName: string,
  newName: string,
): string | undefined {
  if (newVar) return newVar
  if (!oldVar) return undefined
  const mapped = oldVar === 'cli' ? providerCliValue : oldVar
  logToFile(
    `DEPRECATED: ${legacyName}=${oldVar} → use ${newName}=${mapped} instead`,
  )
  return mapped
}

const parsedConfig = Config.safeParse({
  openaiApiKey: process.env.OPENAI_API_KEY,
  geminiApiKey: process.env.GEMINI_API_KEY,
  deepseekApiKey: process.env.DEEPSEEK_API_KEY,
  defaultModel: process.env.CONSULT_LLM_DEFAULT_MODEL,
  geminiBackend: resolvedGeminiBackend,
  openaiBackend: resolvedOpenaiBackend,
  codexReasoningEffort: migratePrefixedEnv(
    process.env.CONSULT_LLM_CODEX_REASONING_EFFORT,
    process.env.CODEX_REASONING_EFFORT,
    'CODEX_REASONING_EFFORT',
    'CONSULT_LLM_CODEX_REASONING_EFFORT',
  ),
  systemPromptPath: process.env.CONSULT_LLM_SYSTEM_PROMPT_PATH,
})

if (!parsedConfig.success) {
  const details = parsedConfig.error.issues
    .map((issue) => `  ${issue.path.join('.')}: ${issue.message}`)
    .join('\n')
  const msg = `Invalid environment variables:\n${details}`
  logToFile(`FATAL ERROR:\n${msg}`)
  console.error(`❌ ${msg}`)
  process.exit(1)
}

export const config: Config = {
  ...parsedConfig.data,
  allowedModels: enabledModels,
}
