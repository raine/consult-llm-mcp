---
name: review
description: Collect critical feedback from all registered LLMs on an artifact (architecture doc, implementation, plan). Intellectual debate with push-back — no sycophancy. Reports findings and unresolved disagreements.
allowed-tools: Bash, Glob, Grep, Read, Write
disable-model-invocation: true
---

Collect critical, honest feedback from all LLMs on an artifact. Push back on weak arguments. Report both consensus findings and unresolved disagreements.

## Phase 0: Load `consult-llm` skill

**Load the `consult-llm` skill before proceeding** — it defines the invocation contract (stdin heredoc, flags, output format, multi-turn). Do not call the CLI without loading it first.

**Arguments:** `$ARGUMENTS`

Check the arguments for flags:

**Mode flags:**
- `--rounds N` → number of critique rounds (default: 2, max: 3)
- `--dry-run` → skip the final synthesis, just show raw reviews
- `--models <list>` → comma-separated selectors/model IDs to use as reviewers (default: `gemini,openai,anthropic,deepseek`)

Strip all flags from arguments to get the **review target** — a file path, directory, or topic description.

**Set variables:**
- `REVIEWERS`: list of model selectors from `--models` flag, or `["gemini", "openai", "anthropic", "deepseek"]` if omitted
- Build the `-m` flags by repeating `-m <selector>` for each reviewer

## Available Reviewers

Discover which selectors and models are available in this environment:

```
!`consult-llm models`
```

Default reviewers (used when no `--models` flag is given): `gemini`, `openai`, `anthropic`, `deepseek` — all four selectors that have a configured backend.

Override with `--models` flag: `--models gemini,openai` to review with only two, or `--models gemini,openai,anthropic` for three. Any selector or exact model ID from the list above is accepted.

## Critical Rule: No Sycophancy

This skill exists to find problems, not to validate. Instruct every LLM call with:

- **Be critical.** The goal is to find weaknesses, gaps, and risks — not to praise.
- **Disagree openly.** If something looks wrong, say so directly. Do not soften criticism to be polite.
- **Push back.** If another reviewer dismissed a concern too easily, challenge them.
- **Unresolved disagreements are fine.** Not everything needs consensus. Flag genuine disagreements clearly rather than papering over them.

## Phase 1: Read the Target

1. **Parse the arguments** — determine what to review:
   - If it's a file path: read the file(s)
   - If it's a directory: explore and read key files
   - If it's a topic/description: gather relevant files from the codebase

2. **Gather context** — use Glob, Grep, Read to understand:
   - The artifact itself (full content)
   - Surrounding code/docs it relates to
   - Existing patterns and conventions

3. **Prepare the review brief** — a summary of:
   - What is being reviewed (the artifact and its purpose)
   - Relevant context from the codebase
   - Specific aspects to focus on (if the user mentioned any)

## Phase 2: Independent Reviews

Have all four LLMs independently review the artifact in parallel using a single CLI call.

**Review prompt:**
```
You are a critical reviewer. Your job is to find problems, not to praise.

## What you are reviewing

[Review brief — artifact content and context]

## Your task

Provide a thorough, critical review:

1. **Problems found**: List concrete issues — bugs, logical errors, missing edge cases, architectural flaws, security concerns. Be specific with file paths and line numbers where applicable.
2. **Questionable decisions**: Decisions that might work but deserve scrutiny — are there better alternatives? What are the trade-offs not being considered?
3. **Missing considerations**: What's not addressed that should be? Gaps in error handling, testing, documentation, scalability, maintainability?
4. **Risks**: What could go wrong in production or during maintenance? What assumptions might not hold?
5. **What works well**: (Brief) What's genuinely solid and should be kept as-is?

Rules:
- Be direct and specific. "This could be improved" is useless. "The retry logic on line 45 silently swallows errors, which will make debugging impossible" is useful.
- Do NOT try to be balanced. If you find 10 problems and 1 good thing, report 10 problems and 1 good thing.
- Do NOT soften criticism. If something is bad, say it's bad and explain why.
- Prioritize your findings: critical issues first, minor nits last.
```

Invoke `consult-llm` with `-m <selector>` repeated for each reviewer in REVIEWERS, `--task review`, and `-f <path>` for each relevant file. Send the review prompt on stdin via quoted heredoc. All models are queried in parallel in a single call.

The response is in group format:
- Line 1: `[thread_id:group_xxx]`
- Each model section: `## Model: <id>` header, then `[model:<id>] [thread_id:<per-model-id>]`, then the response body

**Extract thread IDs:** Parse each model's `thread_id` from the per-model header lines. These are needed for Phase 3 since each model receives the other three's responses.

Present all reviews to the user.

## Phase 3: Cross-Review and Push-Back

For each round (default 2, configurable with `--rounds N`, max 3):

Share a combined summary of all other reviewers' findings with each reviewer and ask them to challenge, validate, or push back. Use `-t <thread_id>` to continue each LLM's conversation.

**Cross-review prompt (for each reviewer, include the other reviewers' findings):**
```
The other reviewers provided these assessments:

[Combined summary of the other reviewers' latest responses, labeled by provider name]

Respond critically:

1. **Agree**: Which of their findings are valid? Don't just agree to be agreeable — only agree if you genuinely think they're right.
2. **Disagree**: Which findings are wrong, exaggerated, or missing context? Explain why. If they dismissed one of YOUR concerns, push back if you still think it's valid.
3. **New findings**: Did their reviews make you notice anything you missed?
4. **Priority adjustment**: Given all reviews, what are the TOP 3 most critical issues?

Do NOT be diplomatic. If they're wrong, say they're wrong and explain why. If you change your mind, say so explicitly — don't quietly drop a previous point.
```

Each model receives a different prompt (the other reviewers' responses embedded). Invoke `consult-llm` once with one `--run` flag per reviewer, continuing each model's thread:

```bash
consult-llm \
  --run "model=<selector>,thread=$THREAD,prompt-file=$PROMPT" \
  ...  # one --run per reviewer
  -f <path> ...
```

Write each model's cross-review prompt to a temp file with `mktemp`, using `__CONSULT_LLM_END__` as the heredoc terminator and `>|` to overwrite.

Present all responses to the user after each round.

## Phase 4: Findings Report

**If `--dry-run`:** Present the raw reviews without synthesis.

Analyze all rounds and produce a structured report:

### 1. Categorize findings

Go through every issue raised across all rounds and categorize:

- **Consensus findings**: Majority of reviewers (3+) agree this is a problem
- **Partial consensus**: Two reviewers agree, others disagree or are silent
- **Unresolved disagreements**: Reviewers actively disagree — present all sides fairly
- **Dropped concerns**: Issues raised then abandoned — note why

### 2. Write the report

```markdown
## Review: [Artifact Name]

**Reviewed:** [What was reviewed — file paths or description]
**Reviewers:** Gemini, OpenAI, Anthropic, DeepSeek
**Rounds:** [N]

### Critical Issues (Consensus)

Issues where 3+ reviewers agree, ordered by severity:

1. **[Issue title]**
   - **What:** [Specific description]
   - **Where:** [File:line or section]
   - **Why it matters:** [Impact]
   - **Suggested fix:** [If one emerged from discussion]
   - **Raised by:** [Which reviewers]

### Disputed Issues

Issues where reviewers disagree — all positions presented:

1. **[Issue title]**
   - **For:** [Reviewers and their argument]
   - **Against:** [Reviewers and their argument]
   - **Moderator's take:** [Your assessment of who has the stronger argument]

### Minor Findings

Lower-severity issues and suggestions:
- [Finding 1]
- [Finding 2]

### What's Solid

Aspects reviewers consider well-done:
- [Strength 1]
- [Strength 2]

### Unresolved Questions

Open questions that need human judgment:
- [Question 1]
- [Question 2]
```

### 3. Moderator's assessment

Add your own honest assessment as moderator:
- Which reviewer made the strongest arguments overall?
- Are there issues NO reviewer caught that you noticed?
- What's the single most important thing to address?

Save the report to `history/review-<artifact-name>.md`.

## Critical Rules

- **Independence first.** Phase 2 reviews are fully independent — all models receive the same prompt via parallel `-m` flags in a single call. Do not show one reviewer's output to another until Phase 3.
- **No sycophancy.** Every prompt must instruct LLMs to be critical, disagree openly, and push back. This skill exists to find problems, not to validate.
- **Bash timeout `600000`** on every `consult-llm` call — LLM responses routinely exceed the 2-minute default.
- **Unresolved disagreements are valid output.** Do not force consensus. Flag genuine disagreements clearly rather than papering over them.
- **Verify before adopting.** Findings are claims, not facts. If a reviewer cites a specific file/line, confirm it exists and says what they claim before including it in the report.
- **Phase discipline.** Phase 1 is LLM-free (agent reads and prepares context). Phase 2 is independent LLM work. Phase 3 is adversarial cross-review. Phase 4 is agent synthesis.
