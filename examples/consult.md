Consult an external LLM with the user's query.

User query: `$ARGUMENTS`

When consulting with external LLMs:

**1. Gather context first**

- Use Glob/Grep to find relevant files
- Read the key files
- Select only the files that directly help answer the question

**2. Determine mode and model**

- Web mode: use if the user says "ask in browser" or "consult in browser"
- Codex mode: use if the user says "ask codex"
- Gemini mode: default for "ask gemini"

**3. Call the CLI**

Use `consult-llm` via a quoted heredoc.

Gemini example:

```bash
cat <<'EOF' | consult-llm -m gemini -f "src/file1.rs" -f "src/file2.rs"
<clear, neutral question>
EOF
```

Codex example:

```bash
cat <<'EOF' | consult-llm -m openai -f "src/file1.rs" -f "src/file2.rs"
<clear, neutral question>
EOF
```

Web mode example:

```bash
cat <<'EOF' | consult-llm --web -f "src/file1.rs" -f "src/file2.rs"
<clear, neutral question>
EOF
```

**4. Present results**

- API mode: summarize the answer's key insights and recommendations
- Web mode: tell the user the prompt was copied to the clipboard and ask them to paste the browser LLM response back

**Critical rules**

- Always gather file context first
- Ask neutral, open-ended questions
- Provide focused, relevant files
