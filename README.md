# consult-llm-mcp

An MCP server that lets Claude Code consult stronger AI models (GPT-5.4, Gemini
3.1 Pro, Claude Opus 4.7, DeepSeek V4 Pro, MiniMax M2.7) when Sonnet has you running in circles and you need
to bring in the heavy artillery. Supports multi-turn conversations.

```
> Still getting this error after your fix. Ask gemini
  E5108: Error executing lua: attempt to call method 'child' (a nil value)

⏺ Let me consult Gemini to understand the Neovim treesitter API changes:

⏺ consult-llm:consult_llm (MCP)(prompt: "Neovim plugin broken due to treesitter
                                API changes...", files: ["lua/testonly.lua"],
                                model: "gemini-2.5-pro")
  ⎿ The issue is that iter_matches() can return nil nodes in newer Neovim
    versions. Switch to iter_captures() which never returns nil nodes, and use
    named_child(0) instead of child(0) to get only meaningful syntax nodes.
    … +93 lines (ctrl+r to expand)

⏺ The LLM identified the issue - iter_matches() API changed. Switching to
  iter_captures() approach now.
```

[Quick start](#quick-start) · [Configuration](#configuration) ·
[Skills](#skills) · [Monitor TUI](#monitor) · [Why MCP?](#why-mcp-and-not-cli) ·
[Changelog](CHANGELOG.md)

## Features

- Query powerful AI models (GPT-5.4, Gemini 3.1 Pro, Claude Opus 4.7, DeepSeek
  V4 Pro, MiniMax M2.7) with
  relevant files as context
- Include git changes for code review
- Comprehensive logging with cost estimation (if using API)
- [Monitor TUI](#monitor): Real-time dashboard for watching active consultations
- [Gemini CLI backend](#gemini-cli): Use the `gemini` CLI for Gemini models
- [Codex CLI backend](#codex-cli): Use the `codex` CLI for OpenAI models
- [Cursor CLI backend](#cursor-cli): Use the `cursor-agent` CLI to route GPT and
  Gemini models through a single tool
- [OpenCode CLI backend](#opencode-cli): Use `opencode` CLI with Copilot, OpenRouter,
  or any of 75+ providers
- [Multi-turn conversations](#multi-turn-conversations): Resume CLI sessions
  across requests with `thread_id`
- [Web mode](#web-mode): Copy formatted prompts to clipboard for browser-based
  LLM services
- [Skills](#skills): Multi-LLM debate, collaboration, and consultation workflows
- Less is more: Single MCP tool to not clutter the context

<img src="meta/monitor-screenshot.webp" alt="consult-llm-monitor screenshot" width="600">

## Quick start

1. **Add to Claude Code** (choose one):

   **With npx** (no install required):

   ```bash
   claude mcp add consult-llm \
     -e CONSULT_LLM_GEMINI_BACKEND=gemini-cli \
     -e CONSULT_LLM_OPENAI_BACKEND=codex-cli \
     -- npx -y consult-llm-mcp
   ```

   This is the recommended setup. Uses [Gemini CLI](#gemini-cli) and
   [Codex CLI](#codex-cli). No API keys required, just `gemini login` and
   `codex login`.

   **With binary** (comes with the monitor TUI, no Node.js required):

   ```bash
   curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install.sh | bash
   ```

   ```bash
   claude mcp add consult-llm \
     -e CONSULT_LLM_GEMINI_BACKEND=gemini-cli \
     -e CONSULT_LLM_OPENAI_BACKEND=codex-cli \
     -- consult-llm-mcp
   ```

   For global availability across projects, add `--scope user`.

   **Using API keys instead of CLI backends:**

   ```bash
   claude mcp add consult-llm \
     -e OPENAI_API_KEY=your_openai_key \
     -e GEMINI_API_KEY=your_gemini_key \
     -e ANTHROPIC_API_KEY=your_anthropic_key \
     -e DEEPSEEK_API_KEY=your_deepseek_key \
     -e MINIMAX_API_KEY=your_minimax_key \
     -- npx -y consult-llm-mcp
   ```

2. **Verify connection** with `/mcp`:

   ```
   ❯ 1. consult-llm            ✔ connected
   ```

3. **Ask a question**:
   > "Consult Gemini about how to fix the race condition in server.ts"

## Example workflows

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

<details>
<summary>Using slash command to consult multiple LLMs for comparison</summary>

```
> /consult The branch completions should dynamic. Git should be called only when
using tab to autocomplete. Ask gemini and codex

⏺ I'll consult both Gemini and Codex about making the branch completions dynamic (only
  calling git when tab-completing). Let me gather the relevant context first.

⏺ consult-llm - consult_llm (MCP)(prompt: "I have a Rust CLI tool using clap that needs dynamic shell completions. Currently, the branch completions are generated at completion script
                                 generation time via `possible_values()`. Instead, I need the git operations to only run when the user presses TAB to autocomplete.\n\nCurrent setup:\n1.
                                 `WorktreeBranchParser` implements `TypedValueParser` with `possible_values()` that calls git\n2. Shell completions are generated via clap_complete's
                                 `generate()` function\n3. The completions are sourced at shell startup: `source <(workmux completions zsh)`\n\nThe problem: Running `workmux completions
                                 zsh` takes ~210ms because it runs git operations during completion generation, not during actual tab completion.\n\nHow can I implement truly dynamic
                                 completions where git is only called when the user presses TAB? What's the best approach with clap and clap_complete for zsh?", model:
                                 "gemini-3-pro-preview", files: ["/Users/raine/code/workmux/src/cli.rs","/Users/raine/code/workmux/src/git.rs"], web_mode: false)
  ⎿ The root cause of the slow startup (~210ms) is that `clap_complete::generate()` executes your `possible_values()` logic (git operations) *while generating the script*. This bake
    s the *current* branches into the static script, which is both slow (happens every time you `source` it) and incorrect (branches become stale).
    … +116 lines (ctrl+o to expand)


⏺ consult-llm - consult_llm (MCP)(prompt: "I have a Rust CLI tool using clap that needs dynamic shell completions. Currently, the branch completions are generated at completion script
                                 generation time via `possible_values()`. Instead, I need the git operations to only run when the user presses TAB to autocomplete.\n\nCurrent setup:\n1.
                                 `WorktreeBranchParser` implements `TypedValueParser` with `possible_values()` that calls git\n2. Shell completions are generated via clap_complete's
                                 `generate()` function\n3. The completions are sourced at shell startup: `source <(workmux completions zsh)`\n\nThe problem: Running `workmux completions
                                 zsh` takes ~210ms because it runs git operations during completion generation, not during actual tab completion.\n\nHow can I implement truly dynamic
                                 completions where git is only called when the user presses TAB? What's the best approach with clap and clap_complete for zsh?", model:
                                 "gpt-5.1-codex-max", files: ["/Users/raine/code/workmux/src/cli.rs","/Users/raine/code/workmux/src/git.rs"], web_mode: false)
  ⎿ **Findings**
    - `WorktreeBranchParser::possible_values` (`cli.rs:41-72`) runs git during script generation, so `workmux completions zsh` always pays the git cost (~210ms) even though the resu
    lt is only needed at tab time.
    … +77 lines (ctrl+o to expand)


⏺ Summary from Gemini and Codex

  Both agree on the same solution:

  Root Cause

  clap_complete::generate() calls possible_values() during script generation, not at tab-completion time. This means git runs every shell startup.

...
```

This example shows using the `/consult` slash command to ask multiple LLMs
(Gemini and Codex) about the same problem **in parallel** and compare their
responses. Both LLMs independently arrived at the same solution, providing
confidence in the approach.

</details>

## Backends

Each model is routed to a **backend** — either an API endpoint or a CLI tool.

| Backend        | Description                      | When to use                                                      |
| -------------- | -------------------------------- | ---------------------------------------------------------------- |
| **API**        | Queries LLM APIs directly        | You have API keys and want the simplest setup                    |
| **Gemini CLI** | Shells out to `gemini` CLI       | Free quota (Gemini), existing subscriptions, or prefer CLI tools |
| **Codex CLI**  | Shells out to `codex` CLI        | OpenAI models via Codex subscription                             |
| **Cursor CLI** | Shells out to `cursor-agent` CLI | Route GPT and Gemini through one tool                            |
| **OpenCode CLI** | Shells out to `opencode` CLI   | Use Copilot subscription, OpenCode's 75+ providers               |
| **Web**        | Copies prompt to clipboard       | You prefer browser UIs or want to review prompts                 |

### API (default)

The default backend. Requires API keys configured via environment variables. See
[Configuration](#configuration) for details.

### CLI backends

Instead of making API calls, shell out to local CLI tools. The CLI tools can
explore the codebase themselves, so you don't need to pass all relevant files as
context, but it helps.

#### Gemini CLI

Use Gemini's local CLI to take advantage of Google's
[free quota](https://developers.google.com/gemini-code-assist/resources/quotas#quotas-for-agent-mode-gemini-cli)
or use your Google AI Pro subscription.

**Requirements:**

1. Install the [Gemini CLI](https://github.com/google-gemini/gemini-cli)
2. Authenticate via `gemini login`

**Setup:**

```bash
claude mcp add consult-llm -e CONSULT_LLM_GEMINI_BACKEND=gemini-cli -- npx -y consult-llm-mcp
```

#### Codex CLI

Use OpenAI's Codex CLI for OpenAI models.

**Requirements:**

1. Install the Codex CLI
2. Authenticate via `codex login`

**Setup:**

```bash
claude mcp add consult-llm -e CONSULT_LLM_OPENAI_BACKEND=codex-cli -- npx -y consult-llm-mcp
```

<!-- prettier-ignore -->
> [!TIP]
> Reasoning effort defaults to `high`. Override with
> `-e CONSULT_LLM_CODEX_REASONING_EFFORT=xhigh`. Options: `none`, `minimal`,
> `low`, `medium`, `high`, `xhigh`.

#### Cursor CLI

Use Cursor's agent CLI to route GPT and Gemini models through one tool.

**Requirements:**

1. Install the [Cursor agent CLI](https://cursor.com/cli) (`cursor-agent` in
   PATH)

**Setup:**

```bash
# Route GPT models through Cursor CLI
claude mcp add consult-llm -e CONSULT_LLM_OPENAI_BACKEND=cursor-cli -- npx -y consult-llm-mcp

# Route Gemini models through Cursor CLI
claude mcp add consult-llm -e CONSULT_LLM_GEMINI_BACKEND=cursor-cli -- npx -y consult-llm-mcp

# Route everything through Cursor CLI
claude mcp add consult-llm \
  -e CONSULT_LLM_OPENAI_BACKEND=cursor-cli \
  -e CONSULT_LLM_GEMINI_BACKEND=cursor-cli \
  -e CONSULT_LLM_ALLOWED_MODELS="gemini-3.1-pro-preview,gpt-5.3-codex" \
  -- npx -y consult-llm-mcp
```

**Shell command permissions:**

Cursor CLI runs with `--mode ask`, which blocks shell commands by default. If
your prompts involve tools that need to run commands (e.g. `git diff` for code
review), allow them in `~/.cursor/cli-config.json`:

```json
{
  "permissions": {
    "allow": ["Shell(git diff*)", "Shell(git log*)", "Shell(git show*)"],
    "deny": []
  }
}
```

Glob patterns are supported. The `deny` list takes precedence over `allow`.

#### OpenCode CLI

Use [OpenCode](https://opencode.ai) as a backend to route models through any of
its 75+ supported providers — including GitHub Copilot, OpenRouter, and local
models via Ollama.

**Requirements:**

1. Install [OpenCode](https://opencode.ai/docs/installation/)
2. Configure providers via `opencode providers`

**Setup:**

```bash
# Route MiniMax models through OpenCode
claude mcp add consult-llm \
  -e CONSULT_LLM_MINIMAX_BACKEND=opencode \
  -- npx -y consult-llm-mcp

# Route OpenAI models through Copilot subscription
claude mcp add consult-llm \
  -e CONSULT_LLM_OPENAI_BACKEND=opencode \
  -e CONSULT_LLM_OPENCODE_OPENAI_PROVIDER=copilot \
  -- npx -y consult-llm-mcp

# Route everything through OpenCode
claude mcp add consult-llm \
  -e CONSULT_LLM_OPENAI_BACKEND=opencode \
  -e CONSULT_LLM_GEMINI_BACKEND=opencode \
  -e CONSULT_LLM_DEEPSEEK_BACKEND=opencode \
  -e CONSULT_LLM_MINIMAX_BACKEND=opencode \
  -- npx -y consult-llm-mcp
```

The executor maps model IDs to OpenCode's `provider/model` format automatically.
For example, `MiniMax-M2.7` becomes `opencode run --model minimax/MiniMax-M2.7`.

**Provider prefix overrides:**

By default, each provider family maps to its natural OpenCode provider ID
(`openai`, `google`, `deepseek`, `minimax`). Override with per-family env vars
when you want to route through a different OpenCode provider:

- `CONSULT_LLM_OPENCODE_OPENAI_PROVIDER` — default: `openai`
- `CONSULT_LLM_OPENCODE_GEMINI_PROVIDER` — default: `google`
- `CONSULT_LLM_OPENCODE_DEEPSEEK_PROVIDER` — default: `deepseek`
- `CONSULT_LLM_OPENCODE_MINIMAX_PROVIDER` — default: `minimax`
- `CONSULT_LLM_OPENCODE_PROVIDER` — global fallback for all families

For example, `CONSULT_LLM_OPENCODE_OPENAI_PROVIDER=copilot` turns
`gpt-5.2` into `opencode run --model copilot/gpt-5.2`.

#### Multi-turn conversations

CLI backends support multi-turn conversations via the `thread_id` parameter. The
first response includes a `[thread_id:xxx]` prefix. Pass that ID in follow-up
requests to continue the conversation with full context from prior turns.

This works with all CLI backends (Gemini CLI, Codex CLI, Cursor CLI). Each uses
its own session/thread mechanism, but all are passed through the same
`thread_id` parameter.

```
> Ask codex what's the best caching strategy for our read-heavy API

⏺ consult-llm - consult_llm (MCP)(prompt: "What's the best caching strategy for a read-heavy API?",
                                   model: "gpt-5.3-codex")
  ⎿  [thread_id:thread_b1ff711...]

     Redis with TTL-based invalidation. Set short TTLs for volatile data ...

⏺ Codex recommends Redis with TTL-based invalidation — but our payment
  endpoints need strong consistency. Let me push back on that.

⏺ consult-llm - consult_llm (MCP)(prompt: "What if we need strong consistency guarantees?
                                            Does that change your recommendation?",
                                   model: "gpt-5.3-codex",
                                   thread_id: "thread_b1ff711...")
  ⎿  [thread_id:thread_b1ff711...]

     Yes — with strong consistency, the TTL approach breaks down. Consider
     write-through caching or cache invalidation on writes instead ...

⏺ Good — Codex revised its recommendation based on the consistency
  constraint. I'll use write-through caching for the payment endpoints.
```

See the [debate skills](#debate-skills) for multi-LLM workflows that use
`thread_id` to maintain context across debate rounds.

### Web mode

Copies the formatted prompt to clipboard instead of querying an LLM. Paste into
any browser-based LLM (ChatGPT, Claude.ai, Gemini, etc.).

**When to use:** Prefer a specific web UI, want to review the prompt first, or
don't have API keys.

**Workflow:**

1. Ask Claude to "use consult LLM with web mode"
2. Paste into your browser-based LLM
3. Paste the response back into Claude Code

See the "Using web mode..." example above for a concrete transcript.

## Configuration

### Environment variables

- `OPENAI_API_KEY` - Your OpenAI API key (required for OpenAI models in API
  mode)
- `GEMINI_API_KEY` - Your Google AI API key (required for Gemini models in API
  mode)
- `DEEPSEEK_API_KEY` - Your DeepSeek API key (required for DeepSeek models)
- `MINIMAX_API_KEY` - Your MiniMax API key (required for MiniMax models)
- `ANTHROPIC_API_KEY` - Your Anthropic API key (required for Claude models)
- `CONSULT_LLM_DEFAULT_MODEL` - Override the default model (optional)
  - Accepts selectors (`gemini`, `openai`, `anthropic`, `deepseek`, `minimax`)
    or exact model IDs
    (`gpt-5.4`, `gemini-3.1-pro-preview`, `claude-opus-4-7`, etc.)
  - Selectors are resolved to the best available model at startup
- `CONSULT_LLM_GEMINI_BACKEND` - Backend for Gemini models (optional)
  - Options: `api` (default), `gemini-cli`, `cursor-cli`, `opencode`
- `CONSULT_LLM_OPENAI_BACKEND` - Backend for OpenAI models (optional)
  - Options: `api` (default), `codex-cli`, `cursor-cli`, `opencode`
- `CONSULT_LLM_DEEPSEEK_BACKEND` - Backend for DeepSeek models (optional)
  - Options: `api` (default), `opencode`
- `CONSULT_LLM_MINIMAX_BACKEND` - Backend for MiniMax models (optional)
  - Options: `api` (default), `opencode`
- `CONSULT_LLM_ANTHROPIC_BACKEND` - Backend for Anthropic models (optional)
  - Options: `api` (default)
- `CONSULT_LLM_ALLOWED_MODELS` - Restrict which concrete models can be used
  (optional)
  - Comma-separated list, e.g., `gpt-5.4,gemini-3.1-pro-preview`
  - Selectors resolve against this list — e.g., if only `gemini-2.5-pro` is
    allowed, the `gemini` selector resolves to it
  - Useful when a backend doesn't support all models (e.g., Cursor CLI)
  - See [Tips](#controlling-which-models-are-used) for usage examples
- `CONSULT_LLM_EXTRA_MODELS` - Add models not in the built-in list (optional)
  - Comma-separated list, e.g., `grok-3,kimi-k2.5`
  - Merged with built-in models and included in the tool schema
  - Useful for newly released models with a known provider prefix (`gpt-`,
    `gemini-`, `deepseek-`, `MiniMax-`, `claude-`)
- `CONSULT_LLM_CODEX_REASONING_EFFORT` - Configure reasoning effort for Codex
  CLI (optional, default: `high`)
  - See [Codex CLI](#codex-cli) for details and available options
- `CONSULT_LLM_OPENCODE_PROVIDER` - Global OpenCode provider prefix (optional)
  - Overrides the default provider ID for all families when using the `opencode`
    backend
  - See [OpenCode CLI](#opencode-cli) for details and per-family overrides
- `CONSULT_LLM_SYSTEM_PROMPT_PATH` - Custom path to system prompt file
  (optional)
  - Overrides the default `~/.consult-llm-mcp/SYSTEM_PROMPT.md` location
  - Useful for project-specific prompts
- `CONSULT_LLM_NO_UPDATE_CHECK` - Disable automatic update checking on server
  startup (optional)
  - Set to `1` to disable
  - By default, the server checks for new versions in the background every 24
    hours and logs a notice when an update is available
  - Only applies to binary installs — npm installs are never checked
- `MCP_DEBUG_STDIN` - Log raw JSON-RPC messages received on stdin (optional)
  - Set to `1` to enable
  - Logs every message as `RAW RECV` entries and poll timing gaps as
    `STDIN POLL` entries in `mcp.log`
  - Useful for debugging transport-level issues

### Custom system prompt

You can customize the system prompt used when consulting LLMs by creating a
`SYSTEM_PROMPT.md` file in `~/.consult-llm-mcp/`:

```bash
npx consult-llm-mcp init-prompt
```

This creates a placeholder file with the default system prompt that you can edit
to customize how the consultant LLM behaves. The custom prompt is read on every
request, so changes take effect immediately without restarting the server.

When a custom prompt file exists, it acts as a full override — `task_mode`
overlays are not applied on top. To revert to the default prompt with
`task_mode` support, simply delete the `SYSTEM_PROMPT.md` file.

#### Custom prompt path

Use `CONSULT_LLM_SYSTEM_PROMPT_PATH` to override the default prompt file
location. This is useful for project-specific prompts that you can commit to
your repository:

```bash
claude mcp add consult-llm \
  -e GEMINI_API_KEY=your_key \
  -e CONSULT_LLM_SYSTEM_PROMPT_PATH=/path/to/project/.consult-llm-mcp/SYSTEM_PROMPT.md \
  -- npx -y consult-llm-mcp
```

## Tips

### Controlling which models are used

The `model` parameter accepts **selectors** (`gemini`, `openai`, `anthropic`,
`deepseek`) that the server resolves to the best available concrete model. When
no model is specified, the server uses `CONSULT_LLM_DEFAULT_MODEL` or its
built-in fallback.

**Selector resolution order** (first available wins):

| Selector    | Priority                                                       |
| ----------- | -------------------------------------------------------------- |
| `gemini`    | gemini-3.1-pro-preview → gemini-3-pro-preview → gemini-2.5-pro |
| `openai`    | gpt-5.5 → gpt-5.4 → gpt-5.3-codex → gpt-5.2 → gpt-5.2-codex    |
| `anthropic` | claude-opus-4-7                                                |
| `deepseek`  | deepseek-v4-pro                                                |

**Restricting models with `CONSULT_LLM_ALLOWED_MODELS`:**

If your backend doesn't support all models (e.g., Cursor CLI can't use
`gpt-5.4`), use `CONSULT_LLM_ALLOWED_MODELS` to filter. Selectors will
automatically resolve to the best model within the allowed list:

```bash
# Only allow codex models through Cursor CLI
claude mcp add consult-llm \
  -e CONSULT_LLM_OPENAI_BACKEND=cursor-cli \
  -e CONSULT_LLM_ALLOWED_MODELS='gpt-5.3-codex,gemini-3.1-pro-preview' \
  -- npx -y consult-llm-mcp
# "openai" selector → gpt-5.3-codex (gpt-5.4 filtered out)
```

## MCP tool: consult_llm

The server provides a single tool called `consult_llm` for asking powerful AI
models complex questions.

### Parameters

- **prompt** (required): Your question or request for the consultant LLM

- **files** (optional): Array of file paths to include as context
  - All files are added as context with file paths and code blocks

- **model** (optional): Model selector or exact model ID
  - Selectors: `gemini`, `openai`, `anthropic`, `deepseek` — the server resolves
    to the best available model for each family
  - Exact model IDs (`gpt-5.4`, `gemini-3.1-pro-preview`, `claude-opus-4-7`,
    etc.) are also accepted as an advanced override
  - When omitted, the server uses the configured default

- **task_mode** (optional): Controls the system prompt persona. The calling LLM
  should choose based on the task:
  - `general` (default): Neutral base prompt that defers to the user prompt
  - `review`: Critical code reviewer — bugs, security, performance,
    anti-patterns
  - `debug`: Focused troubleshooter — root cause analysis, execution tracing,
    ignores style issues
  - `plan`: Constructive architect — trade-offs, alternatives, always includes a
    final recommendation
  - `create`: Generative writer — docs, content, polished output

- **web_mode** (optional): Copy prompt to clipboard instead of querying LLM
  - Default: `false`
  - When `true`, the formatted prompt (including system prompt and file
    contents) is copied to clipboard for manual pasting into browser-based LLM
    services

- **thread_id** (optional): Resume a multi-turn conversation
  - Works with CLI backends (Codex CLI, Gemini CLI, Cursor CLI)
  - The first response includes a `[thread_id:xxx]` prefix — pass that ID back
    as `thread_id` in follow-up requests to maintain conversation context

- **git_diff** (optional): Include git diff output as context
  - **files** (required): Specific files to include in diff
  - **repo_path** (optional): Path to git repository (defaults to current
    directory)
  - **base_ref** (optional): Git reference to compare against (defaults to HEAD)

## Supported models

- **gemini-2.5-pro**: Google's Gemini 2.5 Pro
- **gemini-3-pro-preview**: Google's Gemini 3 Pro Preview
- **gemini-3.1-pro-preview**: Google's Gemini 3.1 Pro Preview
- **deepseek-v4-pro**: DeepSeek's V4 Pro reasoning model
- **MiniMax-M2.7**: MiniMax's M2.7 reasoning model (204K context)
- **gpt-5.5**: OpenAI's GPT-5.5 model
- **gpt-5.4**: OpenAI's GPT-5.4 model
- **gpt-5.2**: OpenAI's GPT-5.2 model
- **gpt-5.3-codex**: OpenAI's Codex model based on GPT-5.3
- **gpt-5.2-codex**: OpenAI's Codex model based on GPT-5.2
- **claude-opus-4-7**: Anthropic's Claude Opus 4.7

## Logging

All prompts and responses are logged to
`$XDG_STATE_HOME/consult-llm-mcp/mcp.log` (defaults to
`~/.local/state/consult-llm-mcp/mcp.log`) with:

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

## Monitor

`consult-llm-monitor` is a real-time TUI dashboard for watching active
consultations across all running MCP server instances. It shows what's being
consulted, which models are in use, and how long each request takes.

<p align="center">
  <img src="meta/monitor-demo.gif" alt="consult-llm-monitor demo" width="800">
</p>

The monitor binary is included when you install via the install script (same
script that installs `consult-llm-mcp`).

```bash
consult-llm-monitor
```

The main **table view** shows two panels: active server instances with their
in-flight consultations, and a history log of completed consultations with
timestamps, models, durations, and token counts.

Press `Enter` on any consultation to open the **detail view** with the full
event log - prompt, response with syntax-highlighted markdown, tool calls, and
token usage. Press `?` for keyboard shortcuts.

## Activation methods

### 1. No custom activation (simplest)

When you add an MCP to Claude Code, the tool's schema is injected into the
agent's context. This allows Claude to infer when to call the MCP from natural
language (e.g., "ask gemini about..."). Works out of the box, but you have less
control over how the MCP is invoked.

### 2. Skills

Automatically triggers when Claude detects matching intent. Like slash commands,
supports custom instructions (e.g., always gathering relevant files), but not
always reliably triggered. See the [consult skill](#consult) below.

**Recommendation:** Start with no custom activation. Use skills if you need
custom instructions for how the MCP is invoked.

## Skills

### Installing skills

Install all skills globally with a single command:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
```

This installs skills for all detected platforms:

- **Claude Code** → `~/.claude/skills/`
- **OpenCode** → `~/.config/opencode/skills/`
- **Codex** → `~/.codex/skills/`

To uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash -s uninstall
```

### consult

An example [Claude Code skill](https://code.claude.com/docs/en/skills) that uses
the `consult_llm` MCP tool to create commands like "ask gemini" or "ask codex".
See [skills/consult/SKILL.md](skills/consult/SKILL.md) for the full content.

Type "ask gemini about X" or "ask codex about X" in Claude Code. This is not
strictly necessary since Claude can infer from the schema that "ask gemini"
should call this MCP, but it gives more precise control over how the agent calls
this MCP.

### collab

**Collaborative ideation.** Gemini and Codex independently brainstorm ideas,
then build on each other's suggestions across multiple rounds. Unlike debate,
the tone is cooperative — refining and combining rather than critiquing. Claude
synthesizes the strongest ideas into a plan and implements. See
[skills/collab/SKILL.md](skills/collab/SKILL.md).

```
> /collab how should we handle offline sync for the mobile app
```

### collab-vs

**Claude brainstorms with one LLM.** Claude and an opponent (Gemini or Codex)
take turns building on each other's ideas. Like collab, but Claude participates
directly instead of moderating. See
[skills/collab-vs/SKILL.md](skills/collab-vs/SKILL.md).

```
> /collab-vs --gemini how should we handle offline sync for the mobile app
```

### debate

**Claude moderates, two LLMs debate.** Gemini and Codex independently propose
approaches, then critique each other's proposals. Claude synthesizes the best
ideas and implements. See [skills/debate/SKILL.md](skills/debate/SKILL.md).

```
> /debate design the multi-tenant isolation strategy
```

### debate-vs

**Claude participates as a debater** against one opponent LLM (Gemini or Codex)
through multiple rounds. Claude forms its own position, then debates back and
forth before synthesizing and implementing. See
[skills/debate-vs/SKILL.md](skills/debate-vs/SKILL.md).

```
> /debate-vs --gemini design the multi-tenant isolation strategy
```

## Updating

**Binary installs:**

```bash
consult-llm-mcp update
```

Downloads the latest release from GitHub with SHA-256 checksum verification. If
`consult-llm-monitor` is found alongside the binary, it's updated too.

The server also checks for updates in the background on startup (every 24 hours)
and logs a notice when a newer version is available. Disable with
`CONSULT_LLM_NO_UPDATE_CHECK=1`.

## Why MCP and not CLI?

The server maps one `model` parameter onto five backends (OpenAI API, Gemini
API, Gemini CLI, Codex CLI, Cursor CLI) with different commands, streaming
formats, output schemas, file handling, and resume semantics. Doing this through
agent Bash calls would push all of that per-provider plumbing into the agent or
a wrapper script/CLI.

MCP also sidesteps shell escaping. Prompts contain code with backticks, `$`, and
quotes. Passing one model's code-heavy response into another call breaks bash
quoting and requires temp files. MCP passes structured JSON instead.

Multi-turn workflows add more friction as a CLI. To continue a conversation, the
agent needs to find a session ID in the CLI's output and pass it back as a flag
on the next invocation. With MCP, the agent passes `thread_id` as a parameter
and the server handles the provider-specific resume mechanics internally.

The MCP tool is also easier to compose into [skills](#skills). `/consult`,
`/collab`, and `/debate` all just say "call `consult_llm` with these
parameters." A CLI version would need each skill to either teach the agent the
CLI's interface or reference a separate skill that does. A skill that
orchestrates a multi-model debate is ~90 lines with MCP. As shell commands, the
same skill would either balloon into hundreds of lines of escaping rules and
stdout parsing, or depend on another skill that teaches the agent how to call
each CLI.

If you only need a single provider with simple prompts, a Bash call to `gemini`
or `codex` with some `jq` filtering will work fine. MCP starts to make more
sense with multiple backends, multi-turn conversations across providers, or
custom workflows that nicely compose on top.

## Development

To work on the MCP server locally and use your development version:

1. Clone the repository:

   ```bash
   git clone https://github.com/raine/consult-llm-mcp.git
   cd consult-llm-mcp
   ```

2. Build and test:

   ```bash
   cargo build
   cargo test
   just check  # format, lint, test
   ```

3. Add the MCP server to Claude Code using your local build:
   ```bash
   claude mcp add consult-llm -- /path/to/consult-llm-mcp/target/debug/consult-llm-mcp
   ```

Now when you make changes, rebuild with `cargo build` and restart Claude Code.

### Releasing

```bash
scripts/publish patch  # or minor, major
```

This bumps the version in `package.json` and `Cargo.toml`, commits, tags, and
pushes. GitHub Actions handles cross-compilation and npm publishing.

## Related projects

- [workmux](https://github.com/raine/workmux) — Git worktrees + tmux windows for
  parallel AI agent workflows
- [claude-history](https://github.com/raine/claude-history) — Search and view
  Claude Code conversation history with fzf
- [tmux-file-picker](https://github.com/raine/tmux-file-picker) — Pop up fzf in
  tmux to quickly insert file paths, perfect for AI coding assistants
- [tmux-agent-usage](https://github.com/raine/tmux-agent-usage) — Display AI agent
  rate limit usage in your tmux status bar
