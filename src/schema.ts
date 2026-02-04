import { z } from 'zod/v4'
import { ALL_MODELS } from './models.js'
import { SupportedChatModel, fallbackModel } from './config.js'

// Re-export for consumers
export { ALL_MODELS, SupportedChatModel }
export type { SupportedChatModel as SupportedChatModelType }

export const ConsultLlmArgs = z.object({
  files: z
    .array(z.string())
    .optional()
    .describe(
      'Array of file paths to include as context. All files are added as context with file paths and code blocks.',
    ),
  prompt: z
    .string()
    .describe(
      'Your question or request for the consultant LLM. Ask neutral, open-ended questions without suggesting specific solutions to avoid biasing the analysis.',
    ),
  model: SupportedChatModel.optional()
    .default(fallbackModel)
    .describe(
      'LLM model to use. Prefer gpt-5.1-codex-max when user mentions Codex. This parameter is ignored when `web_mode` is `true`.',
    ),
  web_mode: z
    .boolean()
    .optional()
    .default(false)
    .describe(
      "If true, copy the formatted prompt to the clipboard instead of querying an LLM. When true, the `model` parameter is ignored. Use this to paste the prompt into browser-based LLM services. IMPORTANT: Only use this when the user specifically requests it. When true, wait for the user to provide the external LLM's response before proceeding with any implementation.",
    ),
  git_diff: z
    .object({
      repo_path: z
        .string()
        .optional()
        .describe(
          'Path to git repository (defaults to current working directory)',
        ),
      files: z
        .array(z.string())
        .min(1, 'At least one file is required for git diff')
        .describe('Specific files to include in diff'),
      base_ref: z
        .string()
        .optional()
        .default('HEAD')
        .describe(
          'Git reference to compare against (e.g., "HEAD", "main", commit hash)',
        ),
    })
    .optional()
    .describe(
      'Generate git diff output to include as context. Shows uncommitted changes by default.',
    ),
})

const consultLlmInputSchema = z.toJSONSchema(ConsultLlmArgs, {
  target: 'openapi-3.0',
})

export const toolSchema = {
  name: 'consult_llm',
  description: `Ask a more powerful AI for help with complex problems. Provide your question in the prompt field and always include relevant code files as context.

Be specific about what you want: code implementation, code review, bug analysis, architecture advice, etc.

IMPORTANT: Ask neutral, open-ended questions. Avoid suggesting specific solutions or alternatives in your prompt as this can bias the analysis. Instead of "Should I use X or Y approach?", ask "What's the best approach for this problem?" Let the consultant LLM provide unbiased recommendations.`,
  inputSchema: consultLlmInputSchema,
} as const
