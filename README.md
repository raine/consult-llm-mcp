# Consult LLM MCP

An MCP server that lets Claude Code consult stronger AI models (o3, Gemini 2.
Pro, DeepSeek Reasoner) when Sonnet has you running in circles and you need to
bring in the heavy artillery.

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

- Query powerful AI models (o3, Gemini 2.5 Pro, DeepSeek Reasoner) with relevant
  files as context
- Direct queries with optional file context
- Include git changes for code review and analysis
- Comprehensive logging with cost estimation
- Gemini can be used via
  [Gemini CLI](https://github.com/google-gemini/gemini-cli) to take advantage of
  [free quota](https://developers.google.com/gemini-code-assist/resources/quotas#quotas-for-agent-mode-gemini-cli)
- Simple: provides just one MCP tool

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

## Configuration

- `OPENAI_API_KEY` - Your OpenAI API key (required for o3)
- `GEMINI_API_KEY` - Your Google AI API key (required for Gemini models in API
  mode)
- `DEEPSEEK_API_KEY` - Your DeepSeek API key (required for DeepSeek models)
- `CONSULT_LLM_DEFAULT_MODEL` - Override the default model (optional)
  - Options: `o3` (default), `gemini-2.5-pro`, `deepseek-reasoner`
- `GEMINI_MODE` - Choose between API or CLI mode for Gemini models (optional)
  - Options: `api` (default), `cli`
  - CLI mode uses the system-installed `gemini` CLI tool

## MCP Tool: consult_llm

The server provides a single tool called `consult_llm` for asking powerful AI
models complex questions.

### Parameters

- **prompt** (required): Your question or request for the consultant LLM

- **files** (optional): Array of file paths to include as context

  - All files are added as context with file paths and code blocks

- **model** (optional): LLM model to use

  - Options: `o3` (default), `gemini-2.5-pro`, `deepseek-reasoner`

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
