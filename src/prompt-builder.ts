export function buildPrompt(
  markdownFiles: string[],
  otherFiles: Array<{ path: string; content: string }>,
  gitDiffOutput?: string,
): string {
  const promptParts: string[] = []

  if (gitDiffOutput?.trim()) {
    promptParts.push('## Git Diff\n```diff', gitDiffOutput, '```\n')
  }

  if (otherFiles.length > 0) {
    promptParts.push('## Relevant Files\n')
    for (const file of otherFiles) {
      promptParts.push(`### File: ${file.path}`)
      promptParts.push('```')
      promptParts.push(file.content)
      promptParts.push('```\n')
    }
  }

  if (markdownFiles.length > 0) {
    promptParts.push(...markdownFiles)
  }

  return promptParts.join('\n')
}
