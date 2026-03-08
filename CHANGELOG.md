# Changelog

## v2.7.0 (2026-03-08)

- Fixed cursor-agent tool success detection
- Show cursor thinking text content in monitor detail view instead of a static
  "Thinking..." label
- Added PageUp/PageDown support in monitor detail view

## v2.6.0 (2026-03-08)

- Added `consult-llm-monitor` TUI dashboard for real-time monitoring of active
  consultations, with history tracking, detail view, and keyboard navigation

## v2.5.6 (2026-03-07)

- Added bash installer (`curl | bash`) for installing without Node.js

## v2.5.4 (2026-03-07)

- Rewrote server from TypeScript to Rust
- Distributed as cross-compiled native binaries (macOS arm64/x64, Linux
  x64/arm64) via npm with a POSIX sh launcher
- Added `MCP_DEBUG_STDIN` env var for raw stdin transport logging

## v2.5.3 (2026-03-06)

- Added `CONSULT_LLM_` prefix to backend and reasoning effort env vars:
  `CONSULT_LLM_GEMINI_BACKEND`, `CONSULT_LLM_OPENAI_BACKEND`,
  `CONSULT_LLM_CODEX_REASONING_EFFORT`. Old unprefixed names still work with a
  deprecation warning.

## v2.5.2 (2026-03-06)

- Fixed Codex CLI thread resumption failing due to unsupported `--add-dir` flag
  in `codex exec resume`

## v2.5.1 (2026-03-06)

- Consult skill now queries both Gemini and Codex in parallel by default, with
  `--gemini` and `--codex` flags for single-model consultation
- CLI backends now receive main worktree path as additional context when running
  inside a git worktree
- CLI backends now detect external file directories (outside workspace) and pass
  them to Gemini/Codex so referenced files are accessible
- Debate skill now supports multi-round debates via `--rounds` flag (default 2,
  max 3)
- Reduced anchoring bias in debate/debate-vs final review phase
- Added `install-skills` script for easy skill installation

## v2.5.0 (2026-03-05)

- Added gpt-5.4 model support ($2.50/$15 per million tokens)

## v2.4.2 (2026-02-28)

- Added gemini-3.1-pro-preview model support
- Filter unavailable models from the tool schema based on configured API keys
  and CLI backends, preventing errors when selecting unconfigured models

## v2.4.1 (2026-02-25)

- Updated tool description to tell callers not to inline file contents in the
  prompt field, since the server reads files automatically via the `files`
  parameter

## v2.3.0 (2026-02-25)

- Added Cursor CLI (`cursor-agent`) as a new executor backend
- Replaced `GEMINI_MODE`/`OPENAI_MODE` with `GEMINI_BACKEND`/`OPENAI_BACKEND`
  for backend routing (legacy env vars still work with deprecation warnings)
- Added `CONSULT_LLM_EXTRA_MODELS` environment variable for adding models
  without code changes
- Removed gpt-5.1 and Claude models from built-in model list

## v2.2.0 (2026-02-14)

- Added `task_mode` parameter for adaptive system prompts with five modes:
  `review`, `plan`, `create`, `debug`, and `general` (default)

## v2.1.0 (2026-02-12)

- Added multi-turn conversation support for CLI modes via `thread_id` parameter
  - Codex CLI: uses `--json` output and `exec resume` for session continuity
  - Gemini CLI: uses `-o json` output and `-r` flag for session resume
  - Responses include a `[thread_id:xxx]` prefix for follow-up requests
- Replaced generic CLI executor with dedicated Codex and Gemini executors
- Added debate skill example (`skills/debate/SKILL.md`) showcasing multi-turn
  conversations

## v2.0.1 (2026-02-05)

- Added gpt-5.3-codex model support

## v2.0.0 (2026-02-04)

- Log files now stored in XDG state directory
  (`~/.local/state/consult-llm-mcp/`) instead of `~/.consult-llm-mcp/logs/`,
  following the XDG Base Directory Specification

## v1.7.2 (2026-02-04)

- Extracted model definitions to dedicated module, resolving a circular
  dependency between config and schema

## v1.7.1 (2026-02-04)

- Removed o3 model, succeeded by gpt-5.2 which is now the default OpenAI model
- Documented model selection behavior and `CONSULT_LLM_ALLOWED_MODELS` usage

## v1.7.0 (2026-01-29)

- Added configurable system prompt path via `CONSULT_LLM_SYSTEM_PROMPT_PATH`
  environment variable

## v1.5.0 (2026-01-12)

- Added gpt-5.2-codex model support
- Added `CONSULT_LLM_ALLOWED_MODELS` environment variable to filter which models
  are available in the tool schema

## v1.4.7 (2025-12-12)

- Added gpt-5.2 model support

## v1.4.6 (2025-12-03)

- Removed 5-minute timeout from CLI executors

## v1.4.5 (2025-12-02)

- Improved system prompt by removing redundant "no critical issues found"
  statement
- Fixed `init-prompt` command to use npx
- Moved skill and slash command examples to separate files

## v1.4.4 (2025-11-26)

- Added support for gemini-3-pro-preview model

## v1.4.3 (2025-11-22)

- Added `--skip-git-repo-check` to Codex CLI args, fixing issues when running
  outside git repositories

## v1.4.2 (2025-11-21)

- Added gpt-5.1-codex-max model support
- Made Codex CLI reasoning effort configurable
- Fixed Codex CLI execution

## v1.4.1 (2025-11-19)

- Added test suite with vitest
- Fixed web mode file handling
- Split server entry point for better modularity

## v1.4.0 (2025-11-18)

- Added Codex CLI support as a new executor backend
- Refactored LLM execution to a functional executor pattern
- Migrated to Zod v4 with native JSON schema generation
- Added example Claude Code skill to README

## v1.3.0 (2025-11-16)

- Added web mode: copies the formatted prompt to clipboard for pasting into
  browser-based LLMs instead of querying an API directly

## v1.2.0 (2025-10-25)

- Added custom system prompt support, configurable via file
- Added environment variable documentation
- Set up ESLint and code quality checks

## v1.1.2 (2025-10-25)

- Added `--version` flag to display the server version

## v1.1.1 (2025-10-25)

- Added Gemini CLI mode support with a dedicated system prompt and instructions
- Enforced code file context requirement in the tool description
- Improved code review guidance in prompts

## v1.1.0 (2025-07-28)

- Added Gemini CLI mode as an alternative to the Gemini API, with free quota
  support
- Added debug logging and configuration logging with sensitive data redaction

## v1.0.5 (2025-06-29)

- Improved system prompt for more concise and critical analysis
- Updated tool description to emphasize neutral, unbiased questioning
- Moved main prompt to the `prompt` field instead of embedding in markdown

## v1.0.4 (2025-06-25)

- Added system prompt to LLM queries for enhanced analysis and recommendations
- Added `prompt` parameter to the tool

## v1.0.3 (2025-06-23)

- Added deepseek-reasoner and gemini-2.5-pro model options
- Added API key validation checks for OpenAI, Gemini, and DeepSeek models

## v1.0.2 (2025-06-23)

- Added server version logging and version in server metadata

## v1.0.1 (2025-06-23)

Initial release.
