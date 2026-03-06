import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { getMainWorktreePath, _resetCache } from './git-worktree.js'

const execSyncMock = vi.hoisted(() => vi.fn())

vi.mock('child_process', () => ({
  execSync: execSyncMock,
}))

beforeEach(() => {
  _resetCache()
  execSyncMock.mockReset()
})

afterEach(() => {
  _resetCache()
})

describe('getMainWorktreePath', () => {
  it('returns null when git-dir and git-common-dir are identical (main worktree)', () => {
    execSyncMock.mockReturnValue('/repo/.git\n/repo/.git\n')
    expect(getMainWorktreePath()).toBeNull()
  })

  it('returns main worktree path when in a linked worktree', () => {
    execSyncMock.mockReturnValue('/repo/.git/worktrees/my-branch\n/repo/.git\n')
    expect(getMainWorktreePath()).toBe('/repo')
  })

  it('caches the result across calls', () => {
    execSyncMock.mockReturnValue('/repo/.git/worktrees/my-branch\n/repo/.git\n')

    getMainWorktreePath()
    getMainWorktreePath()

    expect(execSyncMock).toHaveBeenCalledTimes(1)
  })

  it('returns null when git command fails', () => {
    execSyncMock.mockImplementation(() => {
      throw new Error('not a git repo')
    })

    expect(getMainWorktreePath()).toBeNull()
  })
})
