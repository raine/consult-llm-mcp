# Consult LLM MCP

An MCP (Model Context Protocol) server that allows you to consult more powerful
AI models with your code and questions.

## Features

- Query powerful AI models (o3, Gemini 2.5 Pro, DeepSeek Reasoner) with file
  context
- Automatic prompt construction from markdown and code files
- Git diff to feed code changes
- Usage tracking with cost estimation
- Comprehensive logging

## Configuration

- `OPENAI_API_KEY` - Your OpenAI API key (required for o3)
- `GEMINI_API_KEY` - Your Google AI API key (required for Gemini models)
- `DEEPSEEK_API_KEY` - Your DeepSeek API key (required for DeepSeek models)
- `CONSULT_LLM_DEFAULT_MODEL` - Override the default model (optional)
  - Options: `o3` (default), `gemini-2.5-pro`, `deepseek-reasoner`

## Usage with Claude Code

### Installation

Add the MCP server to Claude Code:

```bash
claude mcp add consult-llm -- npx -y consult-llm-mcp
```

Or for global availability:

```bash
claude mcp add --scope user consult-llm -- npx -y consult-llm-mcp
```

### Example workflows

Click to expand.

<details>
<summary>Explain the problem, and tell CC to consult a smarter LLM</summary>

```
  <Some long explanation of the issue here>

  Consult LLM for assistance, use gemini

⏺ I see the issue! The problem is that the frontend is trying to use the hint
  for "isn't" but the server generated a hint for "isn't it" (the full phrase).
  This is a mismatch between how the frontend segments text and how the server
  generates hints.

  Let me consult the LLM to get assistance on this segmentation mismatch issue.

⏺ Write(hint-segmentation-issue.md)
  ⎿  Wrote 53 lines to hint-segmentation-issue.md
     # Hint Segmentation Mismatch Issue
     ## Problem Description
     There's a mismatch between how the frontend segments text for clickable
     words and how the backend generates hints. This causes hint unlocking to
     fail when the segments don't match exactly.
     ## Example Case
     **Original Text**: "Hello! Nice weather, isn't it?"
     … +43 lines (ctrl+r to expand)

⏺ consult-llm:consult_llm (MCP)(model: "gemini-2.5-pro", files: ["/project/hint
                                -segmentation-issue.md","/project/web/src/compo
                                nents/HintsText.tsx","/project/web/src/utils/hi
                                nts.ts","/project/server/src/routes/public/hint
                                s.ts","/project/server/src/llm/hints.ts"])
  ⎿ An excellent analysis of the problem. The root cause has been correctly
    identified: a race condition within the `handleConfirmGenerateHints`
    function in `HintsText.tsx`. The component state updates in the background
    after `await generateHints()`, but the function continues executing with a
    stale `activeSegment` variable, causing it to request a hint for a smaller
    text segment ( e.g., "isn't") instead of the full, newly-available hint
    phrase (e.g., "isn't it").
    … +607 lines (ctrl+r to expand)

⏺ Perfect! The LLM identified the core issue - there's a race condition in
  handleConfirmGenerateHints where the function uses stale state after async
  operations. The solution is to re-target the hint key after generation to
  find the correct matching hint.

  Let me implement this solution:

⏺ Update Todos
  ⎿  ☐ Fix hint segmentation mismatch by implementing smarter hint key
       targeting in handleConfirmGenerateHints
```

</details>

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

- Tool call parameters
- Full prompts and responses
- Token usage and cost estimates

<details>
<summary>Example</summary>

```
[2025-06-22T20:16:04.673Z] TOOL CALL: consult_llm
Arguments: {
  "files": [
    "refactor-analysis.md",
    "src/main.ts",
    "src/schema.ts",
    "src/config.ts",
    "src/llm.ts",
    "src/llm-cost.ts"
  ],
  "model": "deepseek-reasoner"
}
================================================================================
[2025-06-22T20:16:04.675Z] PROMPT (model: deepseek-reasoner):
## Relevant Files

### File: src/main.ts

...

Please provide specific suggestions for refactoring with example code structure
where helpful.
================================================================================
[2025-06-22T20:19:20.632Z] RESPONSE (model: deepseek-reasoner):
Based on the analysis, here are the key refactoring suggestions to improve
separation of concerns and maintainability:

...

This refactoring maintains all existing functionality while significantly
improving maintainability and separation of concerns. The new structure makes
it easier to add features like new LLM providers, additional context sources,
or alternative prompt formats.

Tokens: 3440 input, 5880 output | Cost: $0.014769 (input: $0.001892, output: $0.012877)
```

</details>

## CLAUDE.md example

To help Claude Code understand when and how to use this tool, you can add the
following to your project's `CLAUDE.md` file:

````markdown
## consult-llm-mcp

Use the `consult_llm` MCP tool to ask a more powerful AI for help with complex
problems. Write your problem description in a markdown file with as much detail
as possible and pass relevant code files as context. Include files to git_diff
when asking feedback for changes.

Use Gemini 2.5 Pro.

### Example

```bash
echo "<very detailed plan or question to be reviewed by the smart LLM>" > task.md
```

Tool call:

```json
{
  "files": [
    "server/src/db.ts",
    "server/src/routes/conversations.ts",
    "task.md"
  ],
  "git_diff": {
    "files": ["server/src/db.ts", "server/src/routes/conversations.ts"]
  },
  "model": "gemini-2.5-pro"
}
```
````
