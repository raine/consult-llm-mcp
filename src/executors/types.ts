import type OpenAI from 'openai'
import type { SupportedChatModel as SupportedChatModelType } from '../schema.js'

export interface LlmExecutorCapabilities {
  isCli: boolean
  supportsThreads: boolean
  supportsFileRefs: boolean
}

export interface LlmExecutor {
  readonly capabilities: LlmExecutorCapabilities
  execute(
    prompt: string,
    model: SupportedChatModelType,
    systemPrompt: string,
    filePaths?: string[],
    threadId?: string,
  ): Promise<{
    response: string
    usage: OpenAI.CompletionUsage | null
    threadId?: string
  }>
}
