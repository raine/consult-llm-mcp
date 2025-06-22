export interface Config {
  openaiApiKey?: string
  geminiApiKey?: string
}

export const config: Config = {
  openaiApiKey: process.env.OPENAI_API_KEY,
  geminiApiKey: process.env.GEMINI_API_KEY,
}