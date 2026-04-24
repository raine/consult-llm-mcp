# consult-llm

`consult-llm` is a CLI for consulting stronger AI models from your existing
agent workflow. It supports GPT-5.5/5.4, Gemini 3.1 Pro, Claude Opus 4.7,
DeepSeek V4 Pro, and MiniMax M2.7, with API and local CLI backends, multi-turn
threads, git diff context, web-mode clipboard export, and a live monitor TUI.

[Quick start](#quick-start) · [Usage](#usage) · [Configuration](#configuration) · [Skills](#skills) · [Monitor](#monitor) · [Why CLI](#why-cli) · [Migrating from MCP](#migrating-from-mcp) · [Changelog](CHANGELOG.md)

## Features

- Query powerful AI models (GPT-5.5/5.4, Gemini 3.1 Pro, Claude Opus 4.7, DeepSeek V4 Pro, MiniMax M2.7) with relevant file context
- [Gemini CLI backend](#gemini-cli): use the `gemini` CLI for Gemini models
- [Codex CLI backend](#codex-cli): use the `codex` CLI for OpenAI models
- [Cursor CLI backend](#cursor-cli): route GPT and Gemini through `cursor-agent`
- [OpenCode backend](#opencode): use `opencode` with Copilot, OpenRouter, or 75+ providers
- [Multi-turn conversations](#multi-turn-conversations): resume sessions across requests with `thread_id`
- [Web mode](#web-mode): copy formatted prompts to clipboard for browser-based LLMs
- [Skills](#skills): multi-LLM debate, collaboration, and consultation workflows
- [Monitor TUI](#monitor): real-time dashboard for active runs and history

<img src="meta/monitor-screenshot.webp" alt="consult-llm-monitor screenshot" width="600">

## Quick Start

1. Install the binaries:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install.sh | bash
```

2. Pick a backend and scaffold your config:

```bash
consult-llm init-config
```

CLI backends are the easiest to start with if you have an existing subscription (no API key needed):

```bash
consult-llm config set gemini.backend gemini-cli   # requires: gemini login
consult-llm config set openai.backend codex-cli    # requires: codex login
```

Or set API keys as environment variables:

```bash
export OPENAI_API_KEY=your_openai_key
export GEMINI_API_KEY=your_gemini_key
```

3. Install the skills so your agent can call `consult-llm` for you:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
```

Then invoke skills from inside your agent (see [Usage](#usage) right below).

## Usage

The CLI is invoked by your agent via the installed skills; you don't call it directly. From inside Claude Code, OpenCode, or Codex:

```
/consult what's the best way to model this state machine?
/consult --gemini review this design for edge cases
/debate should this be a separate service or stay in the monolith?
```

### CLI utilities

```bash
consult-llm models                    # list available models and resolved selectors
consult-llm doctor                    # diagnose backend auth and config
consult-llm config set <key> <value>  # set a config value (user config by default)
consult-llm init-config               # scaffold ~/.consult-llm/config.yaml
consult-llm init-prompt               # scaffold ~/.consult-llm/SYSTEM_PROMPT.md
consult-llm update                    # self-update the binary
```

`consult-llm doctor` checks that each provider's backend dependency (API key or CLI binary) is satisfied, shows which config files were loaded, and validates session storage. Pass `--verbose` to see all config keys including unset defaults.

## Backends

Each model resolves to a provider backend.

| Backend    | Description                  | When to use                                          |
| ---------- | ---------------------------- | ---------------------------------------------------- |
| API        | Direct provider API calls    | You have API keys and want the simplest setup        |
| Gemini CLI | Shells out to `gemini`       | Free Gemini quota or existing Google tooling         |
| Codex CLI  | Shells out to `codex`        | OpenAI models via Codex subscription                 |
| Cursor CLI | Shells out to `cursor-agent` | Route GPT and Gemini through Cursor                  |
| OpenCode   | Shells out to `opencode`     | Use Copilot, OpenRouter, or other OpenCode providers |

### Gemini CLI

Requirements:

1. Install the [Gemini CLI](https://github.com/google-gemini/gemini-cli)
2. Run `gemini login`

```bash
consult-llm config set gemini.backend gemini-cli
```

### Codex CLI

Requirements:

1. Install Codex CLI
2. Run `codex login`

```bash
consult-llm config set openai.backend codex-cli
consult-llm config set openai.reasoning_effort high  # none | minimal | low | medium | high | xhigh
```

### Cursor CLI

```bash
consult-llm config set openai.backend cursor-cli
consult-llm config set gemini.backend cursor-cli
```

If your prompts need shell commands in Cursor CLI ask mode, allow them in
`~/.cursor/cli-config.json`.

### OpenCode

```bash
consult-llm config set openai.backend opencode
consult-llm config set openai.opencode_provider openai  # optional: override the OpenCode provider
consult-llm config set gemini.backend opencode
consult-llm config set opencode.default_provider copilot  # applies to providers without an override
```

## Configuration

### Config files

consult-llm reads layered YAML config files. Resolution order (highest to lowest precedence):

1. Environment variables
2. `.consult-llm.local.yaml` (project-local overrides, not committed to git)
3. `.consult-llm.yaml` (committed project config)
4. `~/.consult-llm/config.yaml` (user config)

Project files are discovered by walking up from the current directory to the nearest `.git` root or `$HOME`.

Scaffold the user config and set values:

```bash
consult-llm init-config
consult-llm config set default_model gemini
consult-llm config set gemini.backend gemini-cli
# Write to project config instead of user config:
consult-llm config set --project default_model openai
# Write to local project overrides (not committed):
consult-llm config set --local openai.backend codex-cli
```

Values are parsed as YAML, so booleans and lists work naturally:

```bash
consult-llm config set no_update_check true
consult-llm config set allowed_models '[gemini, openai]'
```

Example `~/.consult-llm/config.yaml`:

```yaml
default_model: gemini

gemini:
  backend: gemini-cli

openai:
  backend: codex-cli
  reasoning_effort: high

opencode:
  default_provider: copilot
```

### API keys

API keys cannot go in config files and must be set as environment variables:

- `OPENAI_API_KEY`
- `GEMINI_API_KEY`
- `ANTHROPIC_API_KEY`
- `DEEPSEEK_API_KEY`
- `MINIMAX_API_KEY`

### Custom system prompt

```bash
consult-llm init-prompt   # scaffold ~/.consult-llm/SYSTEM_PROMPT.md
```

Override the path in config:

```yaml
system_prompt_path: /path/to/project/.consult-llm/SYSTEM_PROMPT.md
```

<details>
<summary>All environment variables</summary>

Environment variables override config file values.

| Variable                                 | Description                                                   | Allowed values                                 | Default                           |
| ---------------------------------------- | ------------------------------------------------------------- | ---------------------------------------------- | --------------------------------- |
| `OPENAI_API_KEY`                         | OpenAI API key                                                |                                                |                                   |
| `GEMINI_API_KEY`                         | Gemini API key                                                |                                                |                                   |
| `ANTHROPIC_API_KEY`                      | Anthropic API key                                             |                                                |                                   |
| `DEEPSEEK_API_KEY`                       | DeepSeek API key                                              |                                                |                                   |
| `MINIMAX_API_KEY`                        | MiniMax API key                                               |                                                |                                   |
| `CONSULT_LLM_DEFAULT_MODEL`              | Model or selector to use when `-m` is omitted                 | selector or exact model ID                     | first available                   |
| `CONSULT_LLM_GEMINI_BACKEND`             | Backend for Gemini models                                     | `api` `gemini-cli` `cursor-cli` `opencode`     | `api`                             |
| `CONSULT_LLM_OPENAI_BACKEND`             | Backend for OpenAI models                                     | `api` `codex-cli` `cursor-cli` `opencode`      | `api`                             |
| `CONSULT_LLM_DEEPSEEK_BACKEND`           | Backend for DeepSeek models                                   | `api` `opencode`                               | `api`                             |
| `CONSULT_LLM_MINIMAX_BACKEND`            | Backend for MiniMax models                                    | `api` `opencode`                               | `api`                             |
| `CONSULT_LLM_ANTHROPIC_BACKEND`          | Backend for Anthropic models                                  | `api`                                          | `api`                             |
| `CONSULT_LLM_ALLOWED_MODELS`             | Comma-separated allowlist; restricts which models are enabled | model IDs                                      | all                               |
| `CONSULT_LLM_EXTRA_MODELS`               | Comma-separated extra model IDs to add to the catalog         | model IDs                                      |                                   |
| `CONSULT_LLM_CODEX_REASONING_EFFORT`     | Reasoning effort for Codex CLI backend                        | `none` `minimal` `low` `medium` `high` `xhigh` | `high`                            |
| `CONSULT_LLM_OPENCODE_PROVIDER`          | Default OpenCode provider prefix for all models               | provider name                                  | per-model default                 |
| `CONSULT_LLM_OPENCODE_OPENAI_PROVIDER`   | OpenCode provider for OpenAI models                           | provider name                                  | `openai`                          |
| `CONSULT_LLM_OPENCODE_GEMINI_PROVIDER`   | OpenCode provider for Gemini models                           | provider name                                  | `google`                          |
| `CONSULT_LLM_OPENCODE_DEEPSEEK_PROVIDER` | OpenCode provider for DeepSeek models                         | provider name                                  | `deepseek`                        |
| `CONSULT_LLM_OPENCODE_MINIMAX_PROVIDER`  | OpenCode provider for MiniMax models                          | provider name                                  | `minimax`                         |
| `CONSULT_LLM_SYSTEM_PROMPT_PATH`         | Path to a custom system prompt file                           | file path                                      | `~/.consult-llm/SYSTEM_PROMPT.md` |
| `CONSULT_LLM_NO_UPDATE_CHECK`            | Disable background update checks                              | `1` `true` `yes`                               |                                   |

</details>

## Logging

All prompts and responses are logged to:

```text
$XDG_STATE_HOME/consult-llm/consult-llm.log
```

Default:

```text
~/.local/state/consult-llm/consult-llm.log
```

## Monitor

`consult-llm-monitor` is a real-time TUI for active runs and history.

<p align="center">
  <img src="meta/monitor-demo.gif" alt="consult-llm-monitor demo" width="800">
</p>

```bash
consult-llm-monitor
```

It reads the per-run spool written by `consult-llm`, including active snapshots,
run metadata, event streams, and shared history.

## Skills

### Architecture

The skill system has two layers:

**`consult-llm` (base CLI)** handles the mechanics: reading stdin, attaching file context, calling the right backend, streaming the response, and managing thread IDs for multi-turn conversations. A dedicated `consult-llm` reference skill documents this contract and is loaded by other skills before they invoke the CLI.

**Workflow skills** compose on top. They gather context from the codebase, decide which models to call and how, and synthesize the results for you. When you run `/consult` or `/debate`, the agent reads a skill file that tells it how to orchestrate one or more `consult-llm` calls and what to do with the responses.

### Install

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
```

Platforms supported:

- Claude Code: `~/.claude/skills/`
- OpenCode: `~/.config/opencode/skills/`
- Codex: `~/.codex/skills/`

To uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash -s uninstall
```

### Included skills

- `consult`: ask Gemini, Codex, or both; supports `--gemini`, `--codex`, and `--browser` flags
- `collab`: Gemini and Codex brainstorm together, building on each other's ideas
- `collab-vs`: Claude brainstorms with one opponent LLM in alternating turns
- `debate`: Gemini and Codex propose and critique competing approaches
- `debate-vs`: Claude debates one opponent LLM, then synthesizes the best answer

See `skills/*/SKILL.md` for the exact prompts and invocation patterns.

## Updating

```bash
consult-llm update
```

This downloads the latest GitHub release, verifies its SHA-256 checksum, updates
`consult-llm`, and updates `consult-llm-monitor` if it lives alongside it.

## Migrating from MCP

If you previously used the MCP server version (`consult-llm-mcp` npm package):

1. **Remove the MCP server registration** from your Claude Desktop config (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

   ```json
   // remove this block:
   "mcpServers": {
     "consult-llm": { ... }
   }
   ```

2. **Uninstall the npm package** if you installed it globally:

   ```bash
   npm uninstall -g consult-llm-mcp
   ```

3. **Install the CLI binary** (see [Quick Start](#quick-start)).

4. **Install skills** so your agent can call `consult-llm` for you:

   ```bash
   curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
   ```

5. **Keep your existing env vars:** `OPENAI_API_KEY`, `GEMINI_API_KEY`, etc. are unchanged. You can optionally migrate them to `~/.consult-llm/config.yaml` (see [Config files](#config-files)).

> **Note:** Thread history from the MCP version does not carry over - the CLI uses a different storage format.

## Why CLI

The CLI is easier to use across tmux panes, CI jobs, editors, shell scripts, and
agent skills. A single binary plus stdin/heredoc input works anywhere without an
MCP registration step or per-host tool wiring.

It also keeps the protocol surface small:

- input comes from stdin or `--prompt-file`
- file context uses repeated `-f` flags
- multi-turn state uses `-t <thread_id>`
- clipboard export uses `--web`

That makes it straightforward for agents and humans to call directly.

## Development

```bash
git clone https://github.com/raine/consult-llm-mcp.git
cd consult-llm-mcp
cargo build
cargo test
just check
```

Try the local binary directly:

```bash
cat <<'EOF' | cargo run -- -m gemini
Sanity-check the local build and explain what this CLI does well.
EOF
```

## Releasing

```bash
scripts/publish patch
```

This bumps the workspace version in `Cargo.toml`, optionally updates the
changelog, commits, tags, and pushes. GitHub Actions builds and uploads the
release archives.

## Related Projects

- [workmux](https://github.com/raine/workmux)
- [claude-history](https://github.com/raine/claude-history)
- [tmux-file-picker](https://github.com/raine/tmux-file-picker)
- [tmux-agent-usage](https://github.com/raine/tmux-agent-usage)
