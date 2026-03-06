import { dirname, resolve, relative } from 'node:path'

/**
 * Given a list of absolute file paths, return the unique parent directories
 * of files that live outside `cwd`.  A directory is "external" when the
 * relative path from `cwd` starts with `..`.
 */
export function getExternalDirectories(
  filePaths: string[] | undefined,
  cwd: string = process.cwd(),
): string[] {
  if (!filePaths || filePaths.length === 0) return []

  const dirs = new Set<string>()
  for (const filePath of filePaths) {
    const abs = resolve(filePath)
    const rel = relative(cwd, abs)
    if (rel.startsWith('..')) {
      dirs.add(dirname(abs))
    }
  }
  return [...dirs]
}
