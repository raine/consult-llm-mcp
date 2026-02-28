---
name: consult
description: Consult an external LLM with the user's query.
allowed-tools: Glob, Grep, Read, mcp__consult-llm__consult_llm
---

Consult an external LLM with the user's query.

User query: $ARGUMENTS

When consulting with external LLMs:

**1. Gather Context First**:

- Use Glob/Grep to find relevant files
- Read key files to understand their relevance
- Select files directly related to the question

**2. Determine Mode and Model**:

- **Web mode**: Use if user says "ask in browser" or "consult in browser"
- **Codex mode**: Use if user says "ask codex" → use model "gpt-5.3-codex"
- **Gemini mode**: Default for "ask gemini" → use model "gemini-3.1-pro-preview"

**3. Call the MCP Tool**: Use `mcp__consult-llm__consult_llm` with:

- **For API/CLI mode (Gemini)**:
  - `model`: "gemini-3.1-pro-preview"
  - `prompt`: The user's query, passed through faithfully (see Critical Rules)
  - `files`: Array of relevant file paths

- **For API/CLI mode (Codex)**:
  - `model`: "gpt-5.3-codex"
  - `prompt`: The user's query, passed through faithfully (see Critical Rules)
  - `files`: Array of relevant file paths

- **For web mode**:
  - `web_mode`: true
  - `prompt`: The user's query, passed through faithfully (see Critical Rules)
  - `files`: Array of relevant file paths
  - (model parameter is ignored in web mode)

**4. Present Results**:

- **API mode**: Summarize key insights, recommendations, and considerations from
  the response
- **Web mode**: Inform user the prompt was copied to clipboard and ask them to
  paste it into their browser-based LLM and share the response back

**Critical Rules**:

- ALWAYS gather file context before consulting
- **Pass through the user's query faithfully** — do NOT add your own theories,
  suspects, analysis, or suggested solutions to the prompt. The user's words are
  the prompt. You may lightly rephrase for clarity or add brief factual context
  (e.g. "we recently changed X to Y"), but never inject your own diagnostic
  opinions or hypotheses.
- Provide focused, relevant files (quality over quantity)
