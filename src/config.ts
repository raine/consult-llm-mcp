import { z } from 'zod/v4'
import { SupportedChatModel } from './schema.js'

const Config = z.object({
  openaiApiKey: z.string().optional(),
  geminiApiKey: z.string().optional(),
  deepseekApiKey: z.string().optional(),
  defaultModel: SupportedChatModel.optional(),
  geminiMode: z.enum(['api', 'cli']).default('api'),
  openaiMode: z.enum(['api', 'cli']).default('api'),
  codexReasoningEffort: z
    .enum(['none', 'minimal', 'low', 'medium', 'high', 'xhigh'])
    .optional(),
})

export type Config = z.infer<typeof Config>

const parsedConfig = Config.safeParse({
  openaiApiKey: process.env.OPENAI_API_KEY,
  geminiApiKey: process.env.GEMINI_API_KEY,
  deepseekApiKey: process.env.DEEPSEEK_API_KEY,
  defaultModel: process.env.CONSULT_LLM_DEFAULT_MODEL,
  geminiMode: process.env.GEMINI_MODE,
  openaiMode: process.env.OPENAI_MODE,
  codexReasoningEffort: process.env.CODEX_REASONING_EFFORT,
})

if (!parsedConfig.success) {
  console.error('❌ Invalid environment variables:')
  for (const issue of parsedConfig.error.issues) {
    console.error(`  ${issue.path.join('.')}: ${issue.message}`)
  }
  process.exit(1)
}

export const config = parsedConfig.data
