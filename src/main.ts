#!/usr/bin/env node

import { getClientForModel } from './llm.js'
import { readFileSync, existsSync } from 'fs'
import { resolve } from 'path'

async function main() {
  const args = process.argv.slice(2)
  
  // Check for --dry-run flag
  const dryRunIndex = args.indexOf('--dry-run')
  const isDryRun = dryRunIndex !== -1
  if (isDryRun) {
    args.splice(dryRunIndex, 1) // Remove --dry-run from args
  }

  if (args.length === 0) {
    console.error('Usage: llmtool [options] <prompt or files...>')
    console.error('       llmtool "direct prompt"')
    console.error('       llmtool file1.ts file2.ts prompt.md')
    console.error('       llmtool --dry-run file1.ts prompt.md')
    console.error('')
    console.error('Options:')
    console.error('  --dry-run  Show the prompt without sending to LLM')
    console.error('')
    console.error('Environment variables:')
    console.error('  OPENAI_API_KEY - Required for GPT models')
    console.error('  GEMINI_API_KEY - Required for Gemini models')
    console.error('  LLMTOOL_MODEL - Model to use (default: o3)')
    process.exit(1)
  }

  let prompt: string
  
  // Check if arguments are file paths
  const filesExist = args.every(arg => existsSync(resolve(arg)))
  
  if (filesExist && args.length > 0) {
    // Process files
    const markdownFiles: string[] = []
    const otherFiles: { path: string; content: string }[] = []
    
    for (const arg of args) {
      const filePath = resolve(arg)
      const content = readFileSync(filePath, 'utf-8')
      
      if (arg.endsWith('.md') || arg.endsWith('.markdown')) {
        markdownFiles.push(content)
      } else {
        otherFiles.push({ path: arg, content })
      }
    }
    
    // Build prompt
    let promptParts: string[] = []
    
    // Add non-markdown files as context
    if (otherFiles.length > 0) {
      promptParts.push('## Relevant Files\n')
      for (const file of otherFiles) {
        promptParts.push(`### File: ${file.path}`)
        promptParts.push('```')
        promptParts.push(file.content)
        promptParts.push('```\n')
      }
    }
    
    // Add markdown files as main prompt
    if (markdownFiles.length > 0) {
      promptParts.push(...markdownFiles)
    }
    
    prompt = promptParts.join('\n')
  } else {
    // Treat as direct text prompt
    prompt = args.join(' ')
  }
  
  const model = process.env.LLMTOOL_MODEL || 'o3'
  
  // If dry run, just show the prompt and exit
  if (isDryRun) {
    console.log('=== DRY RUN MODE ===')
    console.log('Model:', model)
    console.log('Prompt length:', prompt.length, 'characters')
    console.log('\n=== PROMPT START ===')
    console.log(prompt)
    console.log('=== PROMPT END ===')
    return
  }

  try {
    const { client } = getClientForModel(model)

    const completion = await client.chat.completions.create({
      model,
      messages: [{ role: 'user', content: prompt }],
    })

    const response = completion.choices[0]?.message?.content
    if (response) {
      console.log(response)
    } else {
      console.error('No response from the model')
      process.exit(1)
    }
  } catch (error) {
    console.error(
      'Error:',
      error instanceof Error ? error.message : String(error),
    )
    process.exit(1)
  }
}

main().catch((error) => {
  console.error('Fatal error:', error)
  process.exit(1)
})
