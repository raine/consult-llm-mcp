import { z } from 'zod/v4'
import { ALL_MODELS } from './models.js'
import { logToFile } from './logger.js'

// Parse allowed models from environment
const rawAllowedModels = process.env.CONSULT_LLM_ALLOWED_MODELS
  ? process.env.CONSULT_LLM_ALLOWED_MODELS.split(',')
      .map((m) => m.trim())
      .filter((m) => m.length > 0)
  : []

const enabledModels =
  rawAllowedModels.length > 0
    ? ALL_MODELS.filter((m) => rawAllowedModels.includes(m))
    : [...ALL_MODELS]

if (enabledModels.length === 0) {
  const msg =
    'Invalid environment variables:\n  CONSULT_LLM_ALLOWED_MODELS: No valid models enabled.'
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
  geminiBackend: migrateBackendEnv(
    process.env.GEMINI_BACKEND,
    process.env.GEMINI_MODE,
    'gemini-cli',
    'GEMINI_MODE',
    'GEMINI_BACKEND',
  ),
  openaiBackend: migrateBackendEnv(
    process.env.OPENAI_BACKEND,
    process.env.OPENAI_MODE,
    'codex-cli',
    'OPENAI_MODE',
    'OPENAI_BACKEND',
  ),
  codexReasoningEffort: process.env.CODEX_REASONING_EFFORT,
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
