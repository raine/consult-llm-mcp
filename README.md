# Consult LLM MCP

An MCP server that lets Claude Code consult stronger AI models (o3, Gemini 2.5
Pro, DeepSeek Reasoner, GPT-5.1 Codex) when Sonnet has you running in circles
and you need to bring in the heavy artillery.

```
This SQL query is timing out on large datasets. Can you help optimize it? Ask Gemini

⏺ I'll examine your query and consult Gemini for database optimization strategies.

⏺ consult-llm:consult_llm (MCP)(prompt: "How can I optimize this slow SQL query
                                for better performance on large datasets?", files: ["queries/user
                                -analytics.sql","schema/tables.sql"], model: "gemini-2.5-pro")
  ⎿ The query has significant performance bottlenecks: missing indexes on join
    columns, inefficient subqueries that could be CTEs, and unnecessary DISTINCT
    operations. Add composite indexes on (user_id, created_at), replace correlated
    subqueries with window functions, and eliminate redundant GROUP BY clauses.
    … +287 lines (ctrl+r to expand)

⏺ Great insights! I'll add the suggested indexes, refactor the subqueries to use
  CTEs with window functions, and optimize the GROUP BY operations.
```

## Features

- Query powerful AI models (o3, Gemini 2.5 Pro, DeepSeek Reasoner, GPT-5.1
  Codex) with relevant files as context
- Direct queries with optional file context
- Include git changes for code review and analysis
- Comprehensive logging with cost estimation
- [Gemini CLI mode](#gemini-cli-mode): Use the `gemini` CLI to take advantage of
  [free quota](https://developers.google.com/gemini-code-assist/resources/quotas#quotas-for-agent-mode-gemini-cli)
- [Codex CLI mode](#codex-cli-mode): Use the `codex` CLI for OpenAI models
- [Web mode](#web-mode): Copy formatted prompts to clipboard for browser-based
  LLM services
- Simple: provides just one MCP tool to not clutter the context

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

Optionally you can provide environment variables directly in the MCP
configuration:

```bash
claude mcp add consult-llm \
  -e OPENAI_API_KEY=your_openai_key \
  -e GEMINI_API_KEY=your_gemini_key \
  -e DEEPSEEK_API_KEY=your_deepseek_key \
  -e GEMINI_MODE=cli \
  -- npx -y consult-llm-mcp
```

### Example workflows

Some real-world examples. Click to expand.

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

<details>
<summary>Using web mode to copy prompts to clipboard for browser-based LLMs</summary>

```
> I need help implementing a -b/--background flag for my Rust CLI. Use consult LLM with web mode

⏺ I'll use web mode to copy the formatted prompt to your clipboard so you can
  paste it into any browser-based LLM service.

⏺ consult-llm:consult_llm (MCP)(prompt: "What is the best approach to implement a
                                 -b/--background flag for the \"workmux add\" command that
                                 creates a tmux window in the background without switching to
                                 it?\n\nThe current implementation automatically switches to
                                 the newly created window via `tmux::select_window()` in the
                                 `setup_environment()` function. I need to make this behavior
                                 conditional based on a flag passed from the
                                 CLI.\n\nConsider:\n1. Where should the background flag be
                                 threaded through the call chain?\n2. Should I modify the
                                 existing SetupOptions struct or add a parameter?\n3. What's
                                 the most maintainable approach given the codebase
                                 structure?\n4. Are there any edge cases to consider (e.g.,
                                 should select_pane also be conditional)?", files:
                                 ["src/cli.rs","src/workflow.rs","src/tmux.rs"], web_mode:
                                 true)
  ⎿  ✓ Prompt copied to clipboard!

     Please paste it into your browser-based LLM service and share the response
     here before I proceed with any implementation.

     Note: File paths were included:
       - src/cli.rs
       - src/workflow.rs
       - src/tmux.rs

> [User pastes the response from the external LLM]

⏺ Thank you! Based on the recommendations, I'll now implement the solution...
```

This is useful when:

- You want to use a free browser-based LLM service instead of API credits
- You prefer a specific LLM's web interface
- You want to review the full prompt before submitting it

</details>

<details>
<summary>Debugging Neovim plugin treesitter API breakage</summary>

````
⏺ Fixed! Added nil checks for both node and child and updated the iter_matches call to handle the newer API properly.

> Still not working:

  E5108: Error executing lua /Users/raine/code/testonly.nvim/lua/testonly.lua:92: attempt to call method 'child' (a nil value)
  stack traceback:
          /Users/raine/code/testonly.nvim/lua/testonly.lua:92: in function 'reset_all_exclusive'
          /Users/raine/code/testonly.nvim/lua/testonly.lua:110: in function 'reset_all_exclusive'
          [string ":lua"]:1: in main chunk

  Ask gemini

⏺ Let me consult with a more powerful AI to understand the current Neovim treesitter API changes:

⏺ consult-llm:consult_llm (MCP)(prompt: "I'm debugging a Neovim plugin that's broken due to treesitter API changes. The error is \"attempt to call method 'child' (a nil value)\" on line 92. The
                               code is trying to access child nodes from treesitter matches but the API seems to have changed in newer Neovim versions.\n\nHere's the problematic
                               code:\n```lua\nfor _, match, _ in parsed_query:iter_matches(root, bufnr, start_row, end_row) do\n    for id, node in pairs(match) do\n        local name =
                               parsed_query.captures[id]\n        if name == \"call\" and node then\n            local child = node:child(0)  -- ERROR HERE\n            if child then\n
                                    local text = vim.treesitter.get_node_text(child, bufnr)\n                -- ... rest of logic\n            end\n        end\n    end\nend\n```\n\nWhat's the
                               correct way to iterate over treesitter query matches and access child nodes in current Neovim versions? What API changes should I be aware of?", files:
                               ["/Users/raine/code/testonly.nvim/lua/testonly.lua"])
  ⎿  Problem
     -------

     … +93 lines (ctrl+r to expand)

⏺ Perfect! The issue is that iter_matches() can return nil nodes in the match table. Let me fix this by switching to the recommended iter_captures() approach:

⏺ Update(lua/testonly.lua)
  ⎿  Updated lua/testonly.lua with 7 additions and 10 removals
       85      local root = parser:parse()[1]:root()
       86      local start_row, _, end_row, _ = root:range()
       87
       88 -    for _, match, _ in parsed_query:iter_matches(root, bufnr, start_row, end_row) do
       89 -      for id, node in pairs(match) do
       90 -        local name = parsed_query.captures[id]
       91 -        if name == "call" and node then
       92 -          local child = node:child(0)
       93 -          if child then
       94 -            local text = vim.treesitter.get_node_text(child, bufnr)
       88 +    for id, node, _ in parsed_query:iter_captures(root, bufnr, start_row, end_row) do
       89 +      if parsed_query.captures[id] == "call" then
       90 +        local first_child = node:named_child(0)
       91 +        if first_child then
       92 +          local text = vim.treesitter.get_node_text(first_child, bufnr)
       93
       94 -            if text == "it.only" or text == "describe.only" then
       95 -              toggle_test_exclusive(child)
       96 -            end
       94 +          if text == "it.only" or text == "describe.only" then
       95 +            toggle_test_exclusive(first_child)
       96            end
       97          end
       98        end

⏺ Fixed! The key changes:

  1. Switched from iter_matches() to iter_captures() which never returns nil nodes
  2. Used named_child(0) instead of child(0) to get only meaningful syntax nodes (not punctuation)
  3. Simplified the iteration logic since iter_captures() directly gives us the node and capture ID

  Your <leader>tr mapping should now work properly with the current Neovim treesitter API.
````

</details>

## Web Mode

When you want Claude Code to prepare the prompt but send it through an LLM web
UI yourself (ChatGPT, Claude.ai, Gemini, etc.), ask it to "use consult LLM with
web mode." Claude will pass `web_mode: true` to `consult_llm`, the MCP will
assemble the full prompt (system prompt + files + git diff), and instead of
hitting an API it copies that text to your clipboard so you can paste it
wherever you like.

- **When to use**: prefer a specific web UI, want to review the prompt first, or
  do not have API keys configured.
- **Workflow**:
  1. Tell Claude Code (or your MCP-aware agent) to "use consult LLM with web
     mode" so it invokes the tool with `web_mode: true`.
  2. Paste the copied prompt into your browser-based LLM and wait for its
     response.
  3. Paste that response back into Claude Code so it can continue.

See the "Using web mode..." example above for a concrete transcript of this
flow.

## Gemini CLI Mode

Use Gemini's local CLI when you want to take advantage of Google's free quota or
keep prompts off the API by enabling CLI mode so consult-llm spawns the `gemini`
binary locally rather than sending the prompt through the API.

- **When to use**: you have the Gemini CLI installed and authenticated, want to
  stay within the CLI's free allowance.
- **Requirements**:
  1. Install the [Gemini CLI](https://github.com/google-gemini/gemini-cli) and
     ensure the `gemini` command is on your `$PATH`.
  2. Authenticate via `gemini login` (and any other setup the CLI requires).
- **Workflow**:
  1. When adding the MCP server, set `GEMINI_MODE=cli`:
     ```bash
     claude mcp add consult-llm \
       -e GEMINI_MODE=cli \
       -- npx -y consult-llm-mcp
     ```
  2. Ask Claude Code to "consult Gemini" (or whichever phrasing you normally
     use). It will call `consult_llm` with the Gemini model, assemble the
     prompt, and shell out to the CLI automatically.

## Codex CLI Mode

Use OpenAI's Codex CLI when you want to use OpenAI models locally through the
CLI instead of making API calls.

- **When to use**: you have the Codex CLI installed and authenticated, prefer to
  use the CLI interface for OpenAI models.
- **Requirements**:
  1. Install the Codex CLI and ensure the `codex` command is on your `$PATH`.
  2. Authenticate via `codex login` (and any other setup the CLI requires).
- **Workflow**:
  1. When adding the MCP server, set `OPENAI_MODE=cli`:
     ```bash
     claude mcp add consult-llm \
       -e OPENAI_MODE=cli \
       -- npx -y consult-llm-mcp
     ```
  2. Ask Claude Code to consult an OpenAI model (like `gpt-5.1-codex`). It will
     call `consult_llm` with the specified model, assemble the prompt, and shell
     out to the Codex CLI automatically.

## Configuration

### Environment Variables

- `OPENAI_API_KEY` - Your OpenAI API key (required for OpenAI models in API
  mode)
- `GEMINI_API_KEY` - Your Google AI API key (required for Gemini models in API
  mode)
- `DEEPSEEK_API_KEY` - Your DeepSeek API key (required for DeepSeek models)
- `CONSULT_LLM_DEFAULT_MODEL` - Override the default model (optional)
  - Options: `o3` (default), `gemini-2.5-pro`, `deepseek-reasoner`,
    `gpt-5.1-codex`, `gpt-5.1-codex-mini`, `gpt-5.1`
- `GEMINI_MODE` - Choose between API or CLI mode for Gemini models (optional)
  - Options: `api` (default), `cli`
  - CLI mode uses the system-installed `gemini` CLI tool
- `OPENAI_MODE` - Choose between API or CLI mode for OpenAI models (optional)
  - Options: `api` (default), `cli`
  - CLI mode uses the system-installed `codex` CLI tool

### Custom System Prompt

You can customize the system prompt used when consulting LLMs by creating a
`SYSTEM_PROMPT.md` file in `~/.consult-llm-mcp/`:

```bash
consult-llm-mcp init-prompt
```

This creates a placeholder file with the default system prompt that you can edit
to customize how the consultant LLM behaves. The custom prompt is read on every
request, so changes take effect immediately without restarting the server.

To revert to the default prompt, simply delete the `SYSTEM_PROMPT.md` file.

## MCP Tool: consult_llm

The server provides a single tool called `consult_llm` for asking powerful AI
models complex questions.

### Parameters

- **prompt** (required): Your question or request for the consultant LLM

- **files** (optional): Array of file paths to include as context

  - All files are added as context with file paths and code blocks

- **model** (optional): LLM model to use

  - Options: `o3` (default), `gemini-2.5-pro`, `deepseek-reasoner`,
    `gpt-5.1-codex`, `gpt-5.1-codex-mini`, `gpt-5.1`

- **web_mode** (optional): Copy prompt to clipboard instead of querying LLM

  - Default: `false`
  - When `true`, the formatted prompt (including system prompt and file
    contents) is copied to clipboard for manual pasting into browser-based LLM
    services

- **git_diff** (optional): Include git diff output as context
  - **files** (required): Specific files to include in diff
  - **repo_path** (optional): Path to git repository (defaults to current
    directory)
  - **base_ref** (optional): Git reference to compare against (defaults to HEAD)

## Supported Models

- **o3**: OpenAI's reasoning model ($2/$8 per million tokens)
- **gemini-2.5-pro**: Google's Gemini 2.5 Pro ($1.25/$10 per million tokens)
- **deepseek-reasoner**: DeepSeek's reasoning model ($0.55/$2.19 per million
  tokens)
- **gpt-5.1-codex**: OpenAI's Codex model optimized for coding
- **gpt-5.1-codex-mini**: Lighter, faster version of gpt-5.1-codex
- **gpt-5.1**: Broad world knowledge with strong general reasoning

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

While not strictly necessary, to help Claude Code understand when and how to use
this tool, you can optionally something like the following to your project's
`CLAUDE.md` file:

```markdown
## consult-llm-mcp

Use the `consult_llm` MCP tool to ask a more powerful AI for help with complex
problems. Include files to git_diff when asking feedback for changes.

Use Gemini 2.5 Pro.

CRITICAL: When asking, don't present options, this will bias the answer.
```

Claude Code seems to know pretty well when to use this MCP even without this
instruction however.

## Example Skill

Here's an example [Claude Code skill](https://code.claude.com/docs/en/skills)
that uses the `consult_llm` MCP tool to create a "ask gemini" command:

```markdown
---
name: gemini-consultant
description: Use it when the user asks to "ask gemini" or "ask in browser"
allowed-tools: Read, Glob, Grep, mcp__consult-llm__consult_llm
---

When consulting with Gemini:

**1. Gather Context First**:

- Use Glob/Grep to find relevant files
- Read key files to understand their relevance
- Select files directly related to the question

**2. Determine Mode**:

- **Web mode**: Use if user says "ask in browser" or "consult in browser"
- **API mode**: Default for direct Gemini API calls

**3. Call the MCP Tool**: Use `mcp__consult-llm__consult_llm` with:

- **For API mode**:

  - `model`: "gemini-2.5-pro"
  - `prompt`: Clear, neutral question without suggesting solutions
  - `files`: Array of relevant file paths

- **For web mode**:
  - `web_mode`: true
  - `prompt`: Clear, neutral question without suggesting solutions
  - `files`: Array of relevant file paths
  - (model parameter is ignored in web mode)

**4. Present Results**:

- **API mode**: Summarize key insights, recommendations, and considerations from
  the response
- **Web mode**: Inform user the prompt was copied to clipboard and ask them to
  paste it into their browser-based LLM and share the response back

**Critical Rules**:

- ALWAYS gather file context before consulting
- Ask neutral, open-ended questions to avoid bias
- Provide focused, relevant files (quality over quantity)
```

Save this as `~/.claude/skills/gemini-consultant/SKILL.md` and you can then use
it by typing "ask gemini about X" in Claude Code.

## Development

To work on the MCP server locally and use your development version:

1. Clone the repository and install dependencies:

   ```bash
   git clone https://github.com/yourusername/consult-llm-mcp.git
   cd consult-llm-mcp
   npm install
   ```

2. Build the project:

   ```bash
   npm run build
   ```

3. Install globally from the local directory:

   ```bash
   npm link
   ```

4. Add the MCP server to Claude Code using the global command:
   ```bash
   claude mcp add consult-llm -- consult-llm-mcp
   ```

Now when you make changes:

1. Rebuild: `npm run build`
2. Restart Claude Code to pick up the changes

Alternatively, you can use the dev script for development without building:

```bash
claude mcp add consult-llm -- npm run dev
```

This runs the TypeScript source directly with `tsx`, allowing faster iteration
without rebuilding.

To unlink the global version later:

```bash
npm unlink -g
```
