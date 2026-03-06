import { describe, it, expect } from 'vitest'
import { getExternalDirectories } from './external-dirs.js'

describe('getExternalDirectories', () => {
  it('returns empty array for no files', () => {
    expect(getExternalDirectories(undefined, '/project')).toEqual([])
    expect(getExternalDirectories([], '/project')).toEqual([])
  })

  it('returns empty array when all files are within cwd', () => {
    expect(
      getExternalDirectories(
        ['/project/src/a.ts', '/project/lib/b.ts'],
        '/project',
      ),
    ).toEqual([])
  })

  it('returns parent directories of external files', () => {
    expect(
      getExternalDirectories(['/other/docs/readme.md'], '/project'),
    ).toEqual(['/other/docs'])
  })

  it('deduplicates directories', () => {
    expect(
      getExternalDirectories(
        ['/other/docs/a.md', '/other/docs/b.md'],
        '/project',
      ),
    ).toEqual(['/other/docs'])
  })

  it('returns multiple directories for files in different locations', () => {
    const result = getExternalDirectories(
      ['/other/a.ts', '/another/b.ts', '/project/c.ts'],
      '/project',
    )
    expect(result).toHaveLength(2)
    expect(result).toContain('/other')
    expect(result).toContain('/another')
  })
})
