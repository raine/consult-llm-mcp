import { z } from 'zod/v4'
import { SupportedChatModel } from './llm.js'

export const ConsultLlmArgs = z.object({
  files: z.array(z.string()).min(1, 'At least one file is required'),
  model: SupportedChatModel.optional(),
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
  description:
    'Ask a more powerful AI for help with complex problems. Write your problem description in a markdown file and pass relevant code files as context.',
  inputSchema: {
    type: 'object',
    properties: {
      files: {
        type: 'array',
        items: { type: 'string' },
        description:
          'Array of file paths to process. Markdown files (.md) become the main prompt, other files are added as context with file paths and code blocks.',
      },
      model: {
        type: 'string',
        enum: ['o3', 'gemini-2.5-pro', 'deepseek-reasoner'],
        default: 'o3',
        description: 'LLM model to use',
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
    required: ['files'],
  },
} as const
