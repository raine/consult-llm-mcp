import { describe, it, expect, vi, beforeEach } from 'vitest'
import { generateGitDiff } from './git.js'

const execSyncMock = vi.hoisted(() => vi.fn())

vi.mock('child_process', () => ({
  execSync: execSyncMock,
}))

beforeEach(() => {
  execSyncMock.mockReset()
})

describe('generateGitDiff', () => {
  it('returns an error string when no files are provided', () => {
    const result = generateGitDiff(undefined, [])
    expect(result).toContain('Error generating git diff')
    expect(result).toContain('No files specified for git diff')
  })

  it('executes git diff with the expected command and options', () => {
    execSyncMock.mockReturnValueOnce('diff output')

    const result = generateGitDiff('/repo', ['a.ts', 'b.ts'], 'main')

    expect(execSyncMock).toHaveBeenCalledWith('git diff main -- a.ts b.ts', {
      cwd: '/repo',
      encoding: 'utf-8',
      maxBuffer: 1024 * 1024,
    })
    expect(result).toBe('diff output')
  })

  it('uses process.cwd() by default when repo path is missing', () => {
    const cwd = process.cwd()
    execSyncMock.mockReturnValueOnce('diff output')

    generateGitDiff(undefined, ['c.ts'])

    expect(execSyncMock).toHaveBeenCalledWith('git diff HEAD -- c.ts', {
      cwd,
      encoding: 'utf-8',
      maxBuffer: 1024 * 1024,
    })
  })

  it('wraps git errors with a helpful prefix', () => {
    execSyncMock.mockImplementationOnce(() => {
      throw new Error('boom')
    })

    const result = generateGitDiff('/repo', ['a.ts'])
    expect(result).toContain('Error generating git diff: boom')
  })
})
