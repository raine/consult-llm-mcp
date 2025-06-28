import { readFileSync, existsSync } from 'fs'
import { resolve } from 'path'

export function processFiles(files: string[]) {
  const resolvedFiles = files.map((f) => resolve(f))
  const missingFiles = resolvedFiles.filter((f) => !existsSync(f))
  if (missingFiles.length > 0) {
    throw new Error(`Files not found: ${missingFiles.join(', ')}`)
  }

  const contextFiles: { path: string; content: string }[] = []

  for (let i = 0; i < files.length; i++) {
    const filePath = resolvedFiles[i]
    const originalPath = files[i]
    const content = readFileSync(filePath, 'utf-8')
    contextFiles.push({ path: originalPath, content })
  }

  return contextFiles
}
