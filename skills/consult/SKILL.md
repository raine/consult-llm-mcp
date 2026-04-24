---
name: consult
description: Consult an external LLM with the user's query.
allowed-tools: Bash, Glob, Grep, Read
---

Consult an external LLM with the user's query via the `consult-llm` CLI.

Load `consult-llm` skill for CLI invocation mechanics.

## Argument handling

**Arguments:** `$ARGUMENTS`

Check `$ARGUMENTS` for flags:

**Reviewer flags** (mutually exclusive):
- `--gemini` → consult only Gemini (`-m gemini`)
- `--codex` → consult only Codex (`-m openai`)
- No flag → consult both Gemini and Codex in parallel (default)

**Mode flags:**
- `--browser` → use web mode (`--web`, copies prompt to clipboard)

Strip all flags from the arguments to get the user query.

## Workflow

### 1. Gather context

- Use Glob/Grep to find relevant files.
- Read key files to confirm relevance.
- Select files directly related to the question — quality over quantity.

### 2. Invoke

**`--gemini`** — single call with `-m gemini`.

**`--codex`** — single call with `-m openai`.

**No flag (default)** — call both Gemini (`-m gemini`) and Codex (`-m openai`), each with `-f <path>` for each relevant file. Never background the calls — wait for each response before proceeding.

**`--browser`** — single call with `--web` (model flag is ignored in web mode).

### 3. Present results

- **Normal mode:** summarize key insights, recommendations, and considerations. When both LLMs were consulted, synthesize — highlight agreements, note disagreements, present a unified takeaway.
- **Web mode:** inform the user the prompt was copied to clipboard and ask them to paste it into their browser-based LLM and share the response back.

## Critical rules

- ALWAYS gather file context before consulting.
- **Pass through the user's query faithfully** — do NOT add your own theories, suspects, analysis, or suggested solutions to the prompt. The user's intent is the prompt. Rephrase as needed so the prompt reads as a direct question to the LLM, not a meta-instruction to you. You may add brief factual context (e.g. "we recently changed X to Y"), but never inject your own diagnostic opinions or hypotheses. Do not pass the user's query verbatim if it is phrased as an instruction to you rather than a question for the LLM.
- Provide focused, relevant files (quality over quantity).
