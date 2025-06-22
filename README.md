# Consult LLM MCP

An MCP (Model Context Protocol) server that allows you to consult more powerful
AI models with your code and questions.

## Features

- Query powerful AI models (o3, Gemini 2.5 Pro, DeepSeek Reasoner) with file
  context
- Automatic prompt construction from markdown and code files
- Git diff integration to show code changes
- Usage tracking with cost estimation
- Comprehensive logging

## Installation

```bash
npm install
npm run build
npm install -g .
```

## Configuration

Set the following environment variables:

- `OPENAI_API_KEY` - Your OpenAI API key (required for o3)
- `GEMINI_API_KEY` - Your Google AI API key (required for Gemini models)
- `DEEPSEEK_API_KEY` - Your DeepSeek API key (required for DeepSeek models)

## Usage with Claude Code

Add the MCP server to Claude Code:

```bash
claude mcp add consult-llm -- consult-llm-mcp
```

Or for global availability:

```bash
claude mcp add --scope user consult-llm -- consult-llm-mcp
```

## MCP Tool: consult_llm

The server provides a single tool called `consult_llm` for asking powerful AI
models complex questions.

### Parameters

- **files** (required): Array of file paths to process

  - Markdown files (.md) become the main prompt
  - Other files are added as context with file paths and code blocks

- **model** (optional): LLM model to use

  - Options: `o3` (default), `gemini-2.5-pro`, `deepseek-reasoner`

- **git_diff** (optional): Include git diff output as context
  - **files** (required): Specific files to include in diff
  - **repo_path** (optional): Path to git repository (defaults to current
    directory)
  - **base_ref** (optional): Git reference to compare against (defaults to HEAD)

### Example Usage

```json
{
  "files": ["src/auth.ts", "src/middleware.ts", "review.md"],
  "model": "o3",
  "git_diff": {
    "files": ["src/auth.ts", "src/middleware.ts"],
    "base_ref": "main"
  }
}
```

## Supported Models

- **o3**: OpenAI's reasoning model ($2/$8 per million tokens)
- **gemini-2.5-pro**: Google's Gemini 2.5 Pro ($1.25/$10 per million tokens)
- **deepseek-reasoner**: DeepSeek's reasoning model ($0.55/$2.19 per million
  tokens)

## Logging

All prompts and responses are logged to `~/.consult-llm-mcp/logs/mcp.log` with:

- Timestamps
- Full prompts and responses
- Token usage and cost estimates

## CLAUDE.md example

To help Claude Code understand when and how to use this tool, you can add the
following to your project's `CLAUDE.md` file:

````markdown
## consult-llm-mcp

Use the `consult_llm` MCP tool to ask a more powerful AI for help with complex
problems. Write your problem description in a markdown file with as much detail
as possible and pass relevant code files as context. Include files to git_diff
when asking feedback for changes.

### Example

\```bash echo
"<very detailed plan or question to be reviewed by the smart LLM>" > task.md
\```

Tool call:

\```json { "files": [ "server/src/db.ts", "server/src/routes/conversations.ts",
"task.md" ], "git_diff": { "files": ["server/src/db.ts",
"server/src/routes/conversations.ts"] } } \```
````

## Development

```bash
# Run in development mode
npm run dev

# Build TypeScript
npm run build

# Format code
npm run format
```
