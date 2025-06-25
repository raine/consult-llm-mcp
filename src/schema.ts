import { z } from 'zod/v4'

export const SupportedChatModel = z.enum([
  'o3',
  'gemini-2.5-pro',
  'deepseek-reasoner',
])

export type SupportedChatModel = z.infer<typeof SupportedChatModel>

export const ConsultLlmArgs = z
  .object({
    files: z.array(z.string()).optional(),
    prompt: z.string().optional(),
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
  .refine(
    (data) => data.files || data.prompt,
    'Either files or prompt must be provided',
  )

export const toolSchema = {
  name: 'consult_llm',
  description: `Ask a more powerful AI for help with complex problems. Write your problem description in a markdown file and pass relevant code files as context. 

Be specific about what you want: code implementation, code review, bug analysis, architecture advice, etc.`,
  inputSchema: {
    type: 'object',
    properties: {
      files: {
        type: 'array',
        items: { type: 'string' },
        description: `Array of file paths to process. Markdown files (.md) become the main prompt, other files are added as context with file paths and code blocks. 

In the markdown file(s), be clear about what you want the LLM to do: implement code, review code, explain concepts, analyze bugs, etc.`,
      },
      prompt: {
        type: 'string',
        description:
          'Direct prompt text for simple questions. Alternative to using markdown files.',
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
    required: [],
  },
} as const
