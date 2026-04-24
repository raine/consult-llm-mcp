# consult-llm

`consult-llm` is a CLI for consulting stronger AI models from your existing
agent workflow. It supports GPT-5.5/5.4, Gemini 3.1 Pro, Claude Opus 4.7,
DeepSeek V4 Pro, and MiniMax M2.7, with API and local CLI backends, multi-turn
threads, git diff context, web-mode clipboard export, and a live monitor TUI.

Installed binaries: `consult-llm`, `consult-llm-monitor` (repo: [consult-llm-mcp](https://github.com/raine/consult-llm-mcp))

[Quick start](#quick-start) · [Usage](#usage) · [Configuration](#configuration) · [Skills](#skills) · [Monitor](#monitor) · [Why CLI](#why-cli) · [Migrating from MCP](#migrating-from-mcp) · [Changelog](CHANGELOG.md)

## Features

- Query powerful AI models with relevant file context
- Route models through API, Gemini CLI, Codex CLI, Cursor CLI, or OpenCode
- Resume conversations with `thread_id`
- Include git diffs for review and debugging
- Copy fully formatted prompts to the clipboard with `--web`
- Watch live runs and history in `consult-llm-monitor`
- Install reusable multi-LLM skills for Claude Code, OpenCode, and Codex

<img src="meta/monitor-screenshot.webp" alt="consult-llm-monitor screenshot" width="600">

## Quick Start

1. Install the binaries:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install.sh | bash
```

2. Pick a backend.

CLI backends are the easiest to start with:

- Gemini models: `gemini login`
- OpenAI models: `codex login`

Common setup:

```bash
export CONSULT_LLM_GEMINI_BACKEND=gemini-cli
export CONSULT_LLM_OPENAI_BACKEND=codex-cli
```

Or use API keys:

```bash
export OPENAI_API_KEY=your_openai_key
export GEMINI_API_KEY=your_gemini_key
export ANTHROPIC_API_KEY=your_anthropic_key
export DEEPSEEK_API_KEY=your_deepseek_key
export MINIMAX_API_KEY=your_minimax_key
```

3. Install the skills so your agent can call `consult-llm` for you:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
```

Then from inside Claude Code, OpenCode, or Codex:

```
/consult what's the best way to model this state machine?
/consult --gemini review this design for edge cases
/debate should this be a separate service or stay in the monolith?
```

## Usage

### Basic ask

```bash
cat <<'EOF' | consult-llm -m openai
What's the best caching strategy for a read-heavy API?
EOF
```

### With files

```bash
cat <<'EOF' | consult-llm -m gemini -f "src/lib.rs" -f "src/cache.rs"
Review this cache invalidation design and point out correctness risks.
EOF
```

### With git diff context

```bash
cat <<'EOF' | consult-llm -m openai --task review \
  --diff-files src/cache.rs \
  --diff-files src/lib.rs \
  --diff-base main
Review these changes for bugs and regressions.
EOF
```

### Multi-turn conversations

The first response prints a prefix like:

```text
[model:gemini-3.1-pro-preview] [thread_id:thread_abc123]
```

Pass that thread ID back with `-t`:

```bash
cat <<'EOF' | consult-llm -m gemini -t "thread_abc123"
What if we need stronger consistency guarantees than your first suggestion assumed?
EOF
```

### Web mode

```bash
cat <<'EOF' | consult-llm --web -f "src/cli.rs" -f "src/workflow.rs"
What's the best way to add a --background flag here?
EOF
```

This copies the formatted prompt to your clipboard for pasting into ChatGPT,
Claude, Gemini, or any browser UI.

### CLI help

```bash
consult-llm --help
consult-llm models
consult-llm doctor
consult-llm init-prompt
consult-llm update
```

### Diagnosing your setup

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
export CONSULT_LLM_GEMINI_BACKEND=gemini-cli
```

### Codex CLI

Requirements:

1. Install Codex CLI
2. Run `codex login`

```bash
export CONSULT_LLM_OPENAI_BACKEND=codex-cli
```

Reasoning effort defaults to `high`. Override with:

```bash
export CONSULT_LLM_CODEX_REASONING_EFFORT=xhigh
```

### Cursor CLI

```bash
export CONSULT_LLM_OPENAI_BACKEND=cursor-cli
export CONSULT_LLM_GEMINI_BACKEND=cursor-cli
```

If your prompts need shell commands in Cursor CLI ask mode, allow them in
`~/.cursor/cli-config.json`.

### OpenCode

```bash
export CONSULT_LLM_OPENAI_BACKEND=opencode
export CONSULT_LLM_GEMINI_BACKEND=opencode
export CONSULT_LLM_DEEPSEEK_BACKEND=opencode
export CONSULT_LLM_MINIMAX_BACKEND=opencode
```

Provider-prefix override env vars:

- `CONSULT_LLM_OPENCODE_OPENAI_PROVIDER`
- `CONSULT_LLM_OPENCODE_GEMINI_PROVIDER`
- `CONSULT_LLM_OPENCODE_DEEPSEEK_PROVIDER`
- `CONSULT_LLM_OPENCODE_MINIMAX_PROVIDER`
- `CONSULT_LLM_OPENCODE_PROVIDER`

## Configuration

### Config files

consult-llm reads layered YAML config files. Resolution order (highest to lowest precedence):

1. Environment variables
2. `.consult-llm.local.yaml` — project-local overrides, not committed to git
3. `.consult-llm.yaml` — committed project config
4. `~/.consult-llm/config.yaml` — user config

Project files are discovered by walking up from the current directory to the nearest `.git` root or `$HOME`.

Scaffold the user config:

```bash
consult-llm init-config
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

### Environment variables (highest precedence)

| Variable                                 | Description                                                   | Allowed values                                 | Default                           |
| ---------------------------------------- | ------------------------------------------------------------- | ---------------------------------------------- | --------------------------------- |
| `OPENAI_API_KEY`                         | OpenAI API key                                                | —                                              | —                                 |
| `GEMINI_API_KEY`                         | Gemini API key                                                | —                                              | —                                 |
| `ANTHROPIC_API_KEY`                      | Anthropic API key                                             | —                                              | —                                 |
| `DEEPSEEK_API_KEY`                       | DeepSeek API key                                              | —                                              | —                                 |
| `MINIMAX_API_KEY`                        | MiniMax API key                                               | —                                              | —                                 |
| `CONSULT_LLM_DEFAULT_MODEL`              | Model or selector to use when `-m` is omitted                 | selector or exact model ID                     | first available                   |
| `CONSULT_LLM_GEMINI_BACKEND`             | Backend for Gemini models                                     | `api` `gemini-cli` `cursor-cli` `opencode`     | `api`                             |
| `CONSULT_LLM_OPENAI_BACKEND`             | Backend for OpenAI models                                     | `api` `codex-cli` `cursor-cli` `opencode`      | `api`                             |
| `CONSULT_LLM_DEEPSEEK_BACKEND`           | Backend for DeepSeek models                                   | `api` `opencode`                               | `api`                             |
| `CONSULT_LLM_MINIMAX_BACKEND`            | Backend for MiniMax models                                    | `api` `opencode`                               | `api`                             |
| `CONSULT_LLM_ANTHROPIC_BACKEND`          | Backend for Anthropic models                                  | `api`                                          | `api`                             |
| `CONSULT_LLM_ALLOWED_MODELS`             | Comma-separated allowlist; restricts which models are enabled | model IDs                                      | all                               |
| `CONSULT_LLM_EXTRA_MODELS`               | Comma-separated extra model IDs to add to the catalog         | model IDs                                      | —                                 |
| `CONSULT_LLM_CODEX_REASONING_EFFORT`     | Reasoning effort for Codex CLI backend                        | `none` `minimal` `low` `medium` `high` `xhigh` | `high`                            |
| `CONSULT_LLM_OPENCODE_PROVIDER`          | Default OpenCode provider prefix for all models               | provider name                                  | per-model default                 |
| `CONSULT_LLM_OPENCODE_OPENAI_PROVIDER`   | OpenCode provider for OpenAI models                           | provider name                                  | `openai`                          |
| `CONSULT_LLM_OPENCODE_GEMINI_PROVIDER`   | OpenCode provider for Gemini models                           | provider name                                  | `google`                          |
| `CONSULT_LLM_OPENCODE_DEEPSEEK_PROVIDER` | OpenCode provider for DeepSeek models                         | provider name                                  | `deepseek`                        |
| `CONSULT_LLM_OPENCODE_MINIMAX_PROVIDER`  | OpenCode provider for MiniMax models                          | provider name                                  | `minimax`                         |
| `CONSULT_LLM_SYSTEM_PROMPT_PATH`         | Path to a custom system prompt file                           | file path                                      | `~/.consult-llm/SYSTEM_PROMPT.md` |
| `CONSULT_LLM_NO_UPDATE_CHECK`            | Disable background update checks                              | `1` `true` `yes`                               | —                                 |

### Custom system prompt

Create the default prompt file:

```bash
consult-llm init-prompt
consult-llm init-config   # scaffold user config file
```

Default location:

```text
~/.consult-llm/SYSTEM_PROMPT.md
```

Override it with:

```bash
export CONSULT_LLM_SYSTEM_PROMPT_PATH=/path/to/project/.consult-llm/SYSTEM_PROMPT.md
```

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

```bash
consult-llm-monitor
```

It reads the per-run spool written by `consult-llm`, including active snapshots,
run metadata, event streams, and shared history.

## Skills

Install all shipped skills globally:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
```

Platforms supported by the installer:

- Claude Code: `~/.claude/skills/`
- OpenCode: `~/.config/opencode/skills/`
- Codex: `~/.codex/skills/`

To uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash -s uninstall
```

Included skills:

- `consult`: ask Gemini, Codex, or browser/web mode through the CLI
- `collab`: Gemini and Codex brainstorm together
- `collab-vs`: Claude brainstorms with one opponent LLM
- `debate`: Gemini and Codex critique competing approaches
- `debate-vs`: Claude debates one opponent LLM

See `skills/*/SKILL.md` for the exact prompts and CLI invocation patterns.

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

5. **Keep your existing env vars** — `OPENAI_API_KEY`, `GEMINI_API_KEY`, etc. are unchanged. You can optionally migrate them to `~/.consult-llm/config.yaml` (see [Config files](#config-files)).

> **Note:** Thread history from the MCP version does not carry over — the CLI uses a different storage format.

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
