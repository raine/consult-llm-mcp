---
name: consult-llm
description: How to invoke the consult-llm CLI. Canonical reference for the invocation contract, flags, stdin/stdout format, and multi-turn. Load this before calling consult-llm from any workflow skill (/consult, /collab, /debate, /collab-vs, /debate-vs).
allowed-tools: Bash
---

Reference for invoking the `consult-llm` CLI. Workflow skills delegate here for mechanics; they focus on orchestration.

## Invocation

Run `consult-llm` with the prompt on **stdin**, using a quoted heredoc.

```bash
cat <<'EOF' | consult-llm -m <selector> -f src/foo.rs -f src/bar.rs
<prompt body>
EOF
```

Rules:

- **ALWAYS use `<<'EOF'` (quoted).** The quotes prevent the shell from expanding `$var`, backticks, or escapes inside the prompt. Unquoted heredocs corrupt prompts.
- **Fallback to `--prompt-file <path>`** if the prompt contains the heredoc terminator, or on Windows/PowerShell. Write the prompt to a temp file, then `consult-llm --prompt-file /tmp/prompt.txt …`.
- **Stdout layout.** First line is `[model:<id>] [thread_id:<id>]`, then a blank line, then the response body. In `--web` mode the prefix is just `[model:<id>]` (no thread).
- **Multi-turn.** Read `[thread_id:xxx]` from line 1 and pass it back with `-t <id>` on the next call. Thread IDs are opaque strings — don't modify them. Not portable across backends.
- **Stderr** carries progress/spinner output. Ignore it.
- **Exit codes.** `0` success, `1` backend/network error (includes thread-not-found), `2` usage error, `3` configuration error (missing API key, unsupported backend).

## Models

Selectors and allowed models resolvable in this environment (availability depends on which API keys are configured):

```
!`consult-llm models`
```

Pass a selector or an exact model ID to `-m`. Only enabled selectors are listed — anything not shown has no available model. **Usually omit `-m`** to use the configured default; pass it explicitly only when the user names a specific model. `-m` is ignored when `--web` is used.

## Task modes

Pick a `--task` mode based on the kind of question. Omit for neutral general-purpose.

| Mode                | When to use                                                                                       |
| ------------------- | ------------------------------------------------------------------------------------------------- |
| `general` (default) | Neutral prompt. Defers to instructions in the prompt body. Use for open questions.                |
| `review`            | Critical code reviewer — bugs, security issues, quality problems.                                 |
| `debug`             | Root-cause troubleshooter from errors/logs/stack traces. Ignores style.                           |
| `plan`              | Constructive architect — explore trade-offs, design solutions. Always ends with a recommendation. |
| `create`            | Generative writer for docs, content, or design output.                                            |

## Web mode

`--web` copies the formatted prompt (system prompt + user prompt + file context) to the clipboard and exits 0 instead of calling an LLM. **Only use when the user specifically asks for browser/web mode.** After invoking, wait for the user to paste the external LLM's response back — do not continue implementation on your own. `-m` is ignored in this mode.

## Prompt authoring

Ask neutral, open-ended questions. Do not suggest specific solutions in the prompt body — that biases the analysis. Let the LLM form its own view.

## Flags

| Flag                         | Purpose                                                         |
| ---------------------------- | --------------------------------------------------------------- |
| `-m, --model <selector\|id>` | See "Models" above. Usually omit.                               |
| `-f, --file <path>`          | Repeatable. File context — path + code block.                   |
| `-t, --thread-id <id>`       | Resume a multi-turn conversation. See "Multi-turn".             |
| `--task <mode>`              | Persona. See "Task modes" above.                                |
| `--web`                      | Clipboard mode. See "Web mode" above.                           |
| `--prompt-file <path>`       | Read prompt from file instead of stdin.                         |
| `--diff-files <path>`        | Repeatable. Include git diff for this file as context.          |
| `--diff-base <ref>`          | Base ref for diff (default `HEAD` — shows uncommitted changes). |
| `--diff-repo <path>`         | Repo path (default cwd).                                        |

Run `consult-llm --help` for the authoritative flag list.
