---
name: collab
description: Gemini and Codex collaboratively brainstorm solutions, building on each other's ideas across rounds. Agent synthesizes the best ideas into a plan.
---

Have Gemini and Codex collaboratively brainstorm solutions, then synthesize the best ideas into a plan. Both LLMs build on each other's ideas across rounds rather than critiquing positions.

**Arguments:** `$ARGUMENTS`

## Phase 1: Understand the Task (No Questions)

1. **Explore the codebase** - use Glob, Grep, Read to understand:
   - Relevant files and their structure
   - Existing patterns and conventions
   - Dependencies and interfaces

2. **Make reasonable assumptions** - do NOT ask clarifying questions
   - Use best judgment based on codebase context
   - Prefer simpler solutions when ambiguous
   - Follow existing patterns in the codebase

3. **Prepare context summary** - create a brief summary of:
   - The task to be implemented
   - Relevant files discovered
   - Key patterns and conventions in the codebase
   - Any constraints or considerations

## Phase 2: Initial Ideas

Have both LLMs independently brainstorm approaches (in parallel).

**Seed prompt:**
```
I need to implement the following task:

[Task description]

Here's what I found in the codebase:
[Context summary - relevant files, patterns, conventions]

Brainstorm implementation ideas:
1. **Ideas**: List 2-3 possible approaches with brief descriptions
2. **Favorite**: Which approach do you lean toward and why?
3. **Open questions**: What aspects are you unsure about or would benefit from another perspective?
4. **Risks**: What could go wrong or be tricky?

Think creatively. Share rough ideas — we're exploring, not committing.
```

Spawn BOTH as parallel subagents (`Agent` tool, `subagent_type: "general-purpose"`, `model: "sonnet"`). NEVER run subagents in the background — always run them in the foreground so you can process their results immediately. Each subagent prompt must include the full seed prompt text and file list so it can make the MCP call independently.

**Gemini subagent** — prompt must include:
- Call `mcp__consult-llm__consult_llm` with `model: "gemini"`, `prompt`: the seed prompt, `files`: [array of relevant source files]
- Return the COMPLETE response including any `[thread_id:xxx]` prefix

**Codex subagent** — prompt must include:
- Call `mcp__consult-llm__consult_llm` with `model: "openai"`, `prompt`: the seed prompt, `files`: [array of relevant source files]
- Return the COMPLETE response including any `[thread_id:xxx]` prefix

**Extract thread IDs:** Save `gemini_thread_id` and `codex_thread_id` from the `[thread_id:xxx]` prefixes in the subagent responses.

Present both sets of ideas to the user.

## Phase 3: Build On Each Other

Each round, share both LLMs' ideas with each other and ask them to build on them (in parallel). Use `thread_id` to continue each LLM's conversation. Continue until the ideas converge into a clear approach — typically 2-3 rounds, but use as many as needed.

**Build-on prompt (same for both, include the other's ideas):**
```
A collaborator shared these ideas:

[Other LLM's response from the previous round]

Build on their thinking:
1. **What resonates**: Which ideas are strong? Why?
2. **Combinations**: Can any ideas be combined into something better?
3. **New ideas**: Did their thinking spark any new approaches?
4. **Refinements**: How would you improve the most promising ideas so far?
5. **Concerns resolved**: Did their ideas address any open questions?

Keep building — don't tear down. Refine toward the best solution.
```

Spawn BOTH as parallel subagents (`Agent` tool, `subagent_type: "general-purpose"`, `model: "sonnet"`). NEVER run subagents in the background — always run them in the foreground so you can process their results immediately. Each subagent prompt must include the full build-on prompt text and thread_id.

**Gemini subagent** — prompt must include:
- Call `mcp__consult-llm__consult_llm` with `model: "gemini"`, `prompt`: build-on prompt with Codex's ideas, `thread_id`: `gemini_thread_id`
- Return the COMPLETE response including any `[thread_id:xxx]` prefix

**Codex subagent** — prompt must include:
- Call `mcp__consult-llm__consult_llm` with `model: "openai"`, `prompt`: build-on prompt with Gemini's ideas, `thread_id`: `codex_thread_id`
- Return the COMPLETE response including any `[thread_id:xxx]` prefix

Present both responses to the user after each round.

**When to stop:** Both LLMs are refining details rather than introducing new ideas, and a clear approach has emerged. Don't stop while there are still unresolved open questions or competing directions.

## Phase 4: Synthesize

After all rounds, synthesize the brainstorm into a plan:

1. **Identify the strongest ideas** — which approaches gained momentum across rounds?

2. **Note convergence** — where did both LLMs naturally align?

3. **Pick the best combination** — merge the strongest elements into one coherent approach

4. **Write the plan**:

````markdown
# [Feature Name] Implementation Plan

**Goal:** [One sentence describing what this builds]

## Brainstorm Summary

**Key ideas from Gemini:** [2-3 bullet points]
**Key ideas from Codex:** [2-3 bullet points]
**Convergence:** [Where they naturally agreed]
**Synthesis:** [How the final approach combines the best of both]

---

### Task 1: [Short description]

**Files:**
- Create: `exact/path/to/file.py`
- Modify: `exact/path/to/existing.py` (lines 123-145)

**Steps:**
1. [Specific action]
2. [Specific action]

**Code:**
```language
// Include actual code, not placeholders
```

---
````

Guidelines:
- **Exact file paths** - never "somewhere in src/"
- **Complete code** - show the actual code
- **Small tasks** - 2-5 minutes of work each
- **DRY, YAGNI** - only what's needed

Save the plan to `history/plan-<feature-name>.md`.
