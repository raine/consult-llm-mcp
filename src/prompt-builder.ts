export function buildPrompt(
  userPrompt: string,
  files: Array<{ path: string; content: string }>,
  gitDiffOutput?: string,
): string {
  const promptParts: string[] = []

  if (gitDiffOutput?.trim()) {
    promptParts.push('## Git Diff\n```diff', gitDiffOutput, '```\n')
  }

  if (files.length > 0) {
    promptParts.push('## Relevant Files\n')
    for (const file of files) {
      promptParts.push(`### File: ${file.path}`)
      promptParts.push('```')
      promptParts.push(file.content)
      promptParts.push('```\n')
    }
  }

  // Add user prompt last
  promptParts.push(userPrompt)

  return promptParts.join('\n')
}
