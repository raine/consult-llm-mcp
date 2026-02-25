import type OpenAI from 'openai'
import type { LlmExecutor } from './types.js'

export function createApiExecutor(client: OpenAI): LlmExecutor {
  return {
    capabilities: {
      isCli: false,
      supportsThreads: false,
      supportsFileRefs: false,
    },

    async execute(prompt, model, systemPrompt, filePaths) {
      if (filePaths && filePaths.length > 0) {
        console.warn(
          `Warning: File paths were provided but are not supported by the API executor for model ${model}. They will be ignored.`,
        )
      }

      const completion = await client.chat.completions.create({
        model,
        messages: [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: prompt },
        ],
      })

      const response = completion.choices[0]?.message?.content
      if (!response) {
        throw new Error('No response from the model via API')
      }

      return { response, usage: completion.usage ?? null }
    },
  }
}
