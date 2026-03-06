import { execSync } from 'node:child_process'
import { resolve } from 'node:path'

let cachedMainWorktreePath: string | null | undefined

/**
 * Detect if we're running inside a git worktree and return the main
 * worktree path.  Returns `null` when we're already in the main worktree
 * (or not in a git repo at all).  The result is cached for the process
 * lifetime since the worktree layout doesn't change at runtime.
 */
export function getMainWorktreePath(): string | null {
  if (cachedMainWorktreePath !== undefined) return cachedMainWorktreePath

  cachedMainWorktreePath = detectMainWorktreePath()
  return cachedMainWorktreePath
}

function detectMainWorktreePath(): string | null {
  try {
    // --git-dir returns the .git path for the current worktree
    // --git-common-dir returns the shared .git path of the main worktree
    // In the main worktree they are identical; in a linked worktree they differ.
    // This works from any subdirectory.
    const output = execSync('git rev-parse --git-dir --git-common-dir', {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()

    const [gitDir, commonDir] = output.split('\n')
    if (!gitDir || !commonDir || gitDir === commonDir) return null

    // The common dir is the .git directory of the main worktree.
    // Its parent is the main worktree root.
    return resolve(commonDir, '..')
  } catch {
    return null
  }
}

/** Reset the cache – useful for tests. */
export function _resetCache(): void {
  cachedMainWorktreePath = undefined
}
