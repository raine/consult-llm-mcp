import clipboard from 'clipboardy'

export async function copyToClipboard(text: string): Promise<void> {
  try {
    await clipboard.write(text)
  } catch (error: unknown) {
    const message =
      error instanceof Error ? error.message : 'Unknown clipboard error'
    throw new Error(`Failed to copy prompt to clipboard: ${message}`)
  }
}
