Consult an external LLM with the user's query.

User query: $ARGUMENTS

When consulting with external LLMs:

**1. Gather Context First**:

- Use Glob/Grep to find relevant files
- Read key files to understand their relevance
- Select files directly related to the question

**2. Determine Mode and Model**:

- **Web mode**: Use if user says "ask in browser" or "consult in browser"
- **Codex mode**: Use if user says "ask codex" → use model "gpt-5.1-codex-max"
- **Gemini mode**: Default for "ask gemini" → use model "gemini-3-pro-preview"

**3. Call the MCP Tool**: Use `mcp__consult-llm__consult_llm` with:

- **For API/CLI mode (Gemini)**:
  - `model`: "gemini-3-pro-preview"
  - `prompt`: Clear, neutral question without suggesting solutions
  - `files`: Array of relevant file paths

- **For API/CLI mode (Codex)**:
  - `model`: "gpt-5.1-codex-max"
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
