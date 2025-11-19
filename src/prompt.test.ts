import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { writeFileSync, mkdirSync, rmSync, readFileSync, chmodSync } from 'fs'
import { join, relative } from 'path'
import { tmpdir } from 'os'
import { processFiles } from './file.js'
import { buildPrompt } from './prompt-builder.js'

describe('processFiles', () => {
  let testDir: string
  let testFile1: string
  let testFile2: string

  beforeEach(() => {
    testDir = join(tmpdir(), `consult-llm-test-${Date.now()}`)
    mkdirSync(testDir, { recursive: true })

    testFile1 = join(testDir, 'test1.ts')
    writeFileSync(testFile1, 'const x = 42;\nexport default x;')

    testFile2 = join(testDir, 'test2.ts')
    writeFileSync(testFile2, 'function hello() {\n  return "world";\n}')
  })

  afterEach(() => {
    rmSync(testDir, { recursive: true, force: true })
  })

  it('processes single file and reads content', () => {
    const files = processFiles([testFile1])

    expect(files).toHaveLength(1)
    expect(files[0]).toMatchObject({
      path: testFile1,
      content: 'const x = 42;\nexport default x;',
    })
  })

  it('processes multiple files in order', () => {
    const files = processFiles([testFile1, testFile2])

    expect(files).toHaveLength(2)
    expect(files[0]).toMatchObject({
      path: testFile1,
      content: readFileSync(testFile1, 'utf-8'),
    })
    expect(files[1]).toMatchObject({
      path: testFile2,
      content: readFileSync(testFile2, 'utf-8'),
    })
  })

  it('throws error when file does not exist', () => {
    const nonExistentFile = join(testDir, 'does-not-exist.ts')

    expect(() => processFiles([nonExistentFile])).toThrow('Files not found')
  })

  it('preserves provided relative paths in the output metadata', () => {
    const relativePath = relative(process.cwd(), testFile1)
    const files = processFiles([relativePath])
    expect(files[0]).toMatchObject({ path: relativePath })
  })

  it('includes duplicate entries when the same file is listed twice', () => {
    const files = processFiles([testFile1, testFile1])
    expect(files).toHaveLength(2)
    expect(files[0]?.content).toBe(files[1]?.content)
  })

  it('surfaces read errors (such as permission issues)', () => {
    try {
      chmodSync(testFile1, 0o000)
      expect(() => processFiles([testFile1])).toThrow()
    } finally {
      chmodSync(testFile1, 0o600)
    }
  })
})

describe('buildPrompt', () => {
  let testDir: string
  let testFile: string

  beforeEach(() => {
    testDir = join(tmpdir(), `consult-llm-test-${Date.now()}`)
    mkdirSync(testDir, { recursive: true })
    testFile = join(testDir, 'example.ts')
    writeFileSync(testFile, 'const example = "test";')
  })

  afterEach(() => {
    rmSync(testDir, { recursive: true, force: true })
  })

  it('inlines file contents with proper formatting', () => {
    const files = processFiles([testFile])
    const prompt = buildPrompt('Test prompt', files)

    expect(prompt).toContain('## Relevant Files')
    expect(prompt).toContain(`### File: ${testFile}`)
    expect(prompt).toContain('const example = "test";')
    expect(prompt).toContain('Test prompt')
  })

  it('includes git diff when provided', () => {
    const prompt = buildPrompt('Test prompt', [], 'diff --git a/file.ts')

    expect(prompt).toContain('## Git Diff')
    expect(prompt).toContain('diff --git a/file.ts')
  })

  it('combines files and git diff in correct order', () => {
    const files = processFiles([testFile])
    const prompt = buildPrompt('Test prompt', files, 'diff content')

    expect(prompt).toContain('## Git Diff')
    expect(prompt).toContain('## Relevant Files')
    expect(prompt).toContain('Test prompt')
  })

  it('returns user prompt when no files or git diff exist', () => {
    const prompt = buildPrompt('Solo prompt', [])
    expect(prompt).toBe('Solo prompt')
  })

  it('ignores empty git diff output', () => {
    const prompt = buildPrompt('Prompt only', [], '   \n  ')
    expect(prompt).not.toContain('## Git Diff')
  })

  it('keeps complex file contents intact', () => {
    const trickyContent = 'const tmpl = `Example ``` snippet```;'
    writeFileSync(testFile, trickyContent)
    const files = processFiles([testFile])
    const prompt = buildPrompt('Prompt', files)
    expect(prompt).toContain(trickyContent)
  })
})
