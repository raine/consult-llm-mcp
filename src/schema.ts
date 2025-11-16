import { z } from 'zod/v4'

export const SupportedChatModel = z.enum([
  'o3',
  'gemini-2.5-pro',
  'deepseek-reasoner',
])

export type SupportedChatModel = z.infer<typeof SupportedChatModel>

export const ConsultLlmArgs = z.object({
  files: z.array(z.string()).optional(),
  prompt: z.string(),
  model: SupportedChatModel.optional(),
  web_mode: z.boolean().optional().default(false),
  git_diff: z
    .object({
      repo_path: z.string().optional(),
      files: z
        .array(z.string())
        .min(1, 'At least one file is required for git diff'),
      base_ref: z.string().optional().default('HEAD'),
    })
    .optional(),
})

export const toolSchema = {
  name: 'consult_llm',
  description: `Ask a more powerful AI for help with complex problems. Provide your question in the prompt field and always include relevant code files as context.

Be specific about what you want: code implementation, code review, bug analysis, architecture advice, etc.

IMPORTANT: Ask neutral, open-ended questions. Avoid suggesting specific solutions or alternatives in your prompt as this can bias the analysis. Instead of "Should I use X or Y approach?", ask "What's the best approach for this problem?" Let the consultant LLM provide unbiased recommendations.`,
  inputSchema: {
    type: 'object',
    properties: {
      files: {
        type: 'array',
        items: { type: 'string' },
        description:
          'Array of file paths to include as context. All files are added as context with file paths and code blocks.',
      },
      prompt: {
        type: 'string',
        description:
          'Your question or request for the consultant LLM. Ask neutral, open-ended questions without suggesting specific solutions to avoid biasing the analysis.',
      },
      model: {
        type: 'string',
        enum: ['o3', 'gemini-2.5-pro', 'deepseek-reasoner'],
        default: 'o3',
        description: 'LLM model to use',
      },
      web_mode: {
        type: 'boolean',
        default: false,
        description:
          "Copy the formatted prompt to clipboard instead of querying the LLM. Use this to paste the prompt into browser-based LLM services. IMPORTANT: When true, wait for the user to provide the external LLM's response before proceeding with any implementation.",
      },
      git_diff: {
        type: 'object',
        properties: {
          repo_path: {
            type: 'string',
            description:
              'Path to git repository (defaults to current working directory)',
          },
          files: {
            type: 'array',
            items: { type: 'string' },
            description: 'Specific files to include in diff',
          },
          base_ref: {
            type: 'string',
            default: 'HEAD',
            description:
              'Git reference to compare against (e.g., "HEAD", "main", commit hash)',
          },
        },
        required: ['files'],
        description:
          'Generate git diff output to include as context. Shows uncommitted changes by default.',
      },
    },
    required: ['prompt'],
  },
} as const
