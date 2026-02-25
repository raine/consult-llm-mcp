import { execSync } from 'node:child_process'

export function generateGitDiff(
  repoPath: string | undefined,
  files: string[],
  baseRef: string = 'HEAD',
): string {
  try {
    const repo = repoPath || process.cwd()
    if (files.length === 0) {
      throw new Error('No files specified for git diff')
    }

    return execSync(`git diff ${baseRef} -- ${files.join(' ')}`, {
      cwd: repo,
      encoding: 'utf-8',
      maxBuffer: 1024 * 1024,
    })
  } catch (error) {
    return `Error generating git diff: ${error instanceof Error ? error.message : String(error)}`
  }
}
