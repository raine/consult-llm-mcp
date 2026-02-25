#!/usr/bin/env node

import { main } from './server.js'
import { logToFile } from './logger.js'

process.on('uncaughtException', (error) => {
  logToFile(`UNCAUGHT EXCEPTION:\n${error.message}\n${error.stack}`)
  console.error('Uncaught exception:', error)
  process.exit(1)
})

process.on('unhandledRejection', (reason) => {
  const message =
    reason instanceof Error
      ? `${reason.message}\n${reason.stack}`
      : String(reason)
  logToFile(`UNHANDLED REJECTION:\n${message}`)
  console.error('Unhandled rejection:', reason)
})

main().catch((error) => {
  const message =
    error instanceof Error ? `${error.message}\n${error.stack}` : String(error)
  logToFile(`FATAL ERROR:\n${message}`)
  console.error('Fatal error:', error)
  process.exit(1)
})
