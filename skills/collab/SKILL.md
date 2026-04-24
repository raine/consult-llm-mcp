---
name: collab
description: Gemini and Codex collaboratively brainstorm solutions, building on each other's ideas across rounds. Agent synthesizes the best ideas into a plan.
---

Have Gemini and Codex collaboratively brainstorm solutions, then synthesize the best ideas into a plan. Both LLMs build on each other's ideas across rounds rather than critiquing positions.

**Arguments:** `$ARGUMENTS`

**No subagents.** Every round is a single `mcp__consult-llm__consult_llm` call
with `model: ["gemini", "openai"]`. Extract `[thread_id:group_xxx]` from the
first line of the response and pass it back as `thread_id` on the next round to
advance both models' conversation state together.

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

Have both LLMs independently brainstorm approaches in one multi-model call.

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

Call `mcp__consult-llm__consult_llm` with:
- `model`: `["gemini", "openai"]`
- `prompt`: the seed prompt
- `files`: [array of relevant source files]

**Extract `group_thread_id`:** Save the `[thread_id:group_xxx]` from the top
line of the response. Use it as `thread_id` on every subsequent round.

Present both sets of ideas to the user.

## Phase 3: Build On Each Other

Each round, show both LLMs' prior responses to both models and ask each to
build on the combined picture. Pass `thread_id: group_thread_id` to continue
both conversations together. Continue until the ideas converge into a clear
approach — typically 2-3 rounds, but use as many as needed.

The build-on prompt is **symmetric**: both models see the same combined view
each round. This loses some "defend your own turf" asymmetry, but gains
simpler control flow and full peer visibility.

**Build-on prompt template (include both prior responses):**
```
Here is how the brainstorm evolved in the last round.

## Gemini's last response
[extracted from previous response's ## Model: gemini section]

## Codex's last response
[extracted from previous response's ## Model: openai section]

Build on this combined picture:
1. **What resonates across both**: Where did you agree? Which ideas are strongest?
2. **Stronger from the other**: Which of the other's ideas are stronger than what you proposed?
3. **Combinations and new ideas**: Can ideas be merged into something better? Did their thinking spark new approaches?
4. **Refinements**: Refine toward the best combined approach.

Keep building — don't tear down.
```

Call `mcp__consult-llm__consult_llm` with:
- `model`: `["gemini", "openai"]`
- `prompt`: the build-on prompt above (with both sections filled in)
- `thread_id`: `group_thread_id`

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
