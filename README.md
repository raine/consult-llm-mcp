# LLM Tool

A simple command-line tool for interacting with OpenAI and Google Gemini APIs using the OpenAI client library.

## Installation

```bash
npm install
npm run build
```

## Configuration

Set the following environment variables:

- `OPENAI_API_KEY` - Your OpenAI API key (required for GPT models)
- `GEMINI_API_KEY` - Your Google AI API key (required for Gemini models)
- `LLMTOOL_MODEL` - The model to use (optional, defaults to `o3`)

## Usage

```bash
# Direct text prompt
export OPENAI_API_KEY=your-openai-key
node dist/main.js "What is the capital of France?"

# With file inputs
node dist/main.js test/user.ts test/auth.ts test/docs/test.md

# Using a specific GPT model
export LLMTOOL_MODEL=gpt-4
node dist/main.js "Explain quantum computing"

# Using Gemini
export GEMINI_API_KEY=your-gemini-key
export LLMTOOL_MODEL=gemini-1.5-flash
node dist/main.js "Write a haiku about programming"

# File input with npm scripts
npm run dev test/user.ts test/auth.ts test/docs/test.md
npm start file1.ts file2.js prompt.md

# Debug mode - show prompt without sending to LLM
npm run dev --dry-run test/user.ts test/docs/test.md
node dist/main.js --dry-run "What is 2+2?"
```

### Options

- `--dry-run` - Show the formatted prompt without sending it to the LLM (useful for debugging)

### File Input Format

When passing files as arguments:
- Non-markdown files (.ts, .js, etc.) are included as "Relevant Files" context
- Markdown files (.md, .markdown) are used as the main prompt
- Files are processed in the order provided
- All arguments must be valid file paths for file mode to activate

## Supported Models

- OpenAI models: `o3`, or any model starting with `gpt-` (e.g., `gpt-3.5-turbo`, `gpt-4`)
- Google models: `gemini-2.5-pro`, or any model starting with `gemini-` (e.g., `gemini-1.5-flash`, `gemini-1.5-pro`)

## Development

```bash
# Run in development mode
npm run dev "Your prompt"

# Build TypeScript
npm run build

# Run built version
npm start "Your prompt"
```