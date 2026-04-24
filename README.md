# consult-llm-mcp

`consult-llm` is a CLI for consulting stronger AI models from your existing
agent workflow. It supports GPT-5.5/5.4, Gemini 3.1 Pro, Claude Opus 4.7,
DeepSeek V4 Pro, and MiniMax M2.7, with API and local CLI backends, multi-turn
threads, git diff context, web-mode clipboard export, and a live monitor TUI.

The GitHub repo is still named `consult-llm-mcp` for now, but the installed
user-facing binaries are:

- `consult-llm`
- `consult-llm-monitor`

[Quick start](#quick-start) · [Usage](#usage) · [Configuration](#configuration) · [Skills](#skills) · [Monitor](#monitor) · [Why CLI](#why-cli) · [Changelog](CHANGELOG.md)

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

3. Run a consultation:

```bash
cat <<'EOF' | consult-llm -m gemini -f "src/main.rs" -f "src/config.rs"
What's the best way to untangle the configuration loading flow here?
EOF
```

4. Optionally install the skills so your agent can call `consult-llm` for you:

```bash
curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install-skills | bash
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

`consult-llm doctor` checks that your environment is wired up correctly:

```
consult-llm v2.13.4 doctor: OK

Providers:
  gemini     gemini-3.1-pro-preview  api        ✓   GEMINI_API_KEY set
  deepseek                                      -   not in allowed_models
  openai     gpt-5.5                 codex-cli  ✓   codex (/opt/homebrew/bin/codex)
  minimax                                       -   not in allowed_models
  anthropic                                     -   not in allowed_models

Config:
  allowed_models          gpt-5.5,gemini-3.1-pro-preview  [~/.consult-llm/config.yaml]
  codex.reasoning_effort  high                      [~/.consult-llm/config.yaml]
  openai.backend          codex-cli                 [~/.consult-llm/config.yaml]

Config files:
  user           ~/.consult-llm/config.yaml  ✓
  project        .consult-llm.yaml           not found
  project-local  .consult-llm.local.yaml     not found

State:
  sessions  ~/.local/state/consult-llm/sessions         ✓ writable
  ...
```

- **Providers** — one row per provider: the model that would be selected, the configured backend, and whether its dependency (API key or CLI binary) is satisfied. Providers excluded by `allowed_models` show as `-`.
- **Config** — non-default config values and which file they came from.
- **Config files** — which config files were found and loaded.
- **State** — session storage directories and their writability.
- **Warnings** — printed at the bottom if anything is broken.

Pass `--verbose` to see all config keys including unset defaults, with raw environment variable names.

## Backends

Each model resolves to a provider backend.

| Backend | Description | When to use |
| --- | --- | --- |
| API | Direct provider API calls | You have API keys and want the simplest setup |
| Gemini CLI | Shells out to `gemini` | Free Gemini quota or existing Google tooling |
| Codex CLI | Shells out to `codex` | OpenAI models via Codex subscription |
| Cursor CLI | Shells out to `cursor-agent` | Route GPT and Gemini through Cursor |
| OpenCode | Shells out to `opencode` | Use Copilot, OpenRouter, or other OpenCode providers |

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

- `OPENAI_API_KEY`
- `GEMINI_API_KEY`
- `DEEPSEEK_API_KEY`
- `MINIMAX_API_KEY`
- `ANTHROPIC_API_KEY`
- `CONSULT_LLM_DEFAULT_MODEL`
- `CONSULT_LLM_GEMINI_BACKEND`
- `CONSULT_LLM_OPENAI_BACKEND`
- `CONSULT_LLM_DEEPSEEK_BACKEND`
- `CONSULT_LLM_MINIMAX_BACKEND`
- `CONSULT_LLM_ANTHROPIC_BACKEND`
- `CONSULT_LLM_ALLOWED_MODELS`
- `CONSULT_LLM_EXTRA_MODELS`
- `CONSULT_LLM_CODEX_REASONING_EFFORT`
- `CONSULT_LLM_OPENCODE_PROVIDER`
- `CONSULT_LLM_SYSTEM_PROMPT_PATH`
- `CONSULT_LLM_NO_UPDATE_CHECK`

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
