export interface Config {
  openaiApiKey?: string
  geminiApiKey?: string
  deepseekApiKey?: string
}

export const config: Config = {
  openaiApiKey: process.env.OPENAI_API_KEY,
  geminiApiKey: process.env.GEMINI_API_KEY,
  deepseekApiKey: process.env.DEEPSEEK_API_KEY,
}
