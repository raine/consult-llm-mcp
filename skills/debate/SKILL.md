---
name: debate
description: Claude debates an opponent LLM (Gemini or Codex) through multi-turn MCP conversation, then synthesizes the best approach and implements.
---

Debate an opponent LLM on the best implementation approach using multi-turn
conversations, then synthesize and implement.

## Configuration

**Arguments:** `$ARGUMENTS`

Check the arguments for flags:

**Opponent flags** (mutually exclusive, exactly one required):

- `--gemini` → debate Gemini (`model`: "gemini-3-pro-preview")
- `--codex` → debate Codex (`model`: "gpt-5.3-codex")

**Mode flags:**

- `--dry-run` → debate and plan only, skip implementation
- `--skip-final` → skip the final review phase
- `--rounds N` → number of debate rounds (default: 1, max: 3). Each round =
  Claude argues + opponent responds.

Strip all flags from arguments to get the task description.

**Set variables based on opponent flag:**

- `OPPONENT`: "Gemini" or "Codex"
- `MODEL`: "gemini-3-pro-preview" or "gpt-5.3-codex"

If neither `--gemini` nor `--codex` is provided, ask the user which opponent to
use.

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

## Phase 2: Opening Arguments

Both debaters propose their approach independently.

### Step 1: Claude's Opening Argument

You ARE the debater. Form your own implementation approach based on what you
found in Phase 1. Write it out in full:

```
## Claude's Opening Argument

1. **Approach**: [2-3 sentences]
2. **Key decisions**: [architectural/design decisions]
3. **Files**: [files to create or modify]
4. **Steps**: [implementation steps]
5. **Trade-offs**: [pros and cons]
```

Present this to the user so they can see your position.

### Step 2: Opponent's Opening Argument

Call `mcp__consult-llm__consult_llm` with:

- `model`: MODEL
- `prompt`: The opening argument prompt below
- `files`: Array of relevant source files discovered in Phase 1

**Opening prompt:**

```
I need to implement the following task:

[Task description]

Here's what I found in the codebase:
[Context summary - relevant files, patterns, conventions]

Propose your implementation approach:
1. **Approach**: Describe your recommended approach in 2-3 sentences
2. **Key decisions**: List the main architectural/design decisions
3. **Files**: What files to create or modify
4. **Steps**: High-level implementation steps
5. **Trade-offs**: What are the pros and cons of this approach?

Be specific and opinionated. Defend your choices.
```

**Extract the thread_id** from the response prefix `[thread_id:xxx]`. Store it
for subsequent rounds.

Present the opponent's opening argument to the user as
`## OPPONENT's Opening Argument`.

## Phase 3: Debate Rounds

For each round (default 1, configurable with `--rounds N`):

### Claude's Turn

Analyze the opponent's latest argument. Write your rebuttal:

```
## Claude's Rebuttal (Round N)

1. **Critique**: [weaknesses in opponent's approach]
2. **Defense**: [address weaknesses opponent identified in your approach]
3. **Concessions**: [good ideas from opponent worth adopting]
4. **Updated position**: [your refined recommendation]
```

Present this to the user.

### Opponent's Turn

Call `mcp__consult-llm__consult_llm` with:

- `model`: MODEL
- `thread_id`: The thread_id from the previous response
- `prompt`: The rebuttal prompt below
- `files`: Array of relevant source files

**Rebuttal prompt:**

```
Your opponent (Claude) has responded with this rebuttal:

[Claude's rebuttal from above]

Provide your counter-argument:
1. **Critique**: What are the weaknesses in Claude's approach and rebuttal?
2. **Defense**: Address the weaknesses Claude identified in your approach
3. **Concessions**: Are there any good ideas from Claude worth adopting?
4. **Updated position**: State your refined recommendation

Be constructive but thorough in your critique.
```

**Update the thread_id** from the response prefix if a new one is returned.

Present the opponent's rebuttal to the user as
`## OPPONENT's Rebuttal (Round N)`.

## Phase 4: Synthesis and Plan

As both debater and moderator, synthesize the final approach:

1. **Score the arguments**:
   - Which approach is simpler?
   - Which approach better fits existing patterns?
   - Which critiques were valid?
   - What concessions were made?

2. **Identify consensus**: Where did you and the opponent agree?

3. **Resolve disagreements**: For each point of contention:
   - Evaluate the arguments from both sides
   - Be honest about where the opponent had the stronger argument
   - Prefer simpler solutions when arguments are equally strong

4. **Write the verdict** as part of the plan:

````markdown
# [Feature Name] Implementation Plan

**Goal:** [One sentence describing what this builds]

## Debate Summary

**Claude's position:** [1-2 sentence summary] **OPPONENT's position:** [1-2
sentence summary]

**Points of agreement:**

- [Consensus point 1]
- [Consensus point 2]

**Resolved disagreements:**

- [Issue]: Claude said X, OPPONENT said Y → **Verdict:** [Decision and why]

**Verdict:** [2-3 sentences on the final synthesized approach]

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
- **Be honest** - credit the opponent when its ideas won

Save the plan to `history/plan-<feature-name>.md`.

## Phase 5: Implement

**If `--dry-run`:** Skip to Phase 7 (Summary) - report the debate and plan
without implementing.

Implement the plan without further interaction:

1. **Follow the plan exactly** - implement each task in order
2. **Commit after each logical unit** - keep commits small and focused
3. **If something is unclear** - make a reasonable decision and note it in the
   commit message
4. **If a task fails** - attempt to fix it before moving on
5. **Only stop if there's a blocking error** that cannot be resolved

Implementation rules:

- Work through tasks sequentially
- Test changes when possible
- Keep commits atomic and well-documented
- Use commit messages that explain the "why"

## Phase 6: Final Review

**If `--skip-final`:** Skip to Phase 7 (Summary).

After implementation, have the opponent review using the existing thread (full
debate context):

Call `mcp__consult-llm__consult_llm` with:

- `model`: MODEL
- `thread_id`: The thread_id from the debate
- `prompt`: Final review prompt below
- `git_diff`: `{ "files": [list of changed files], "base_ref": "HEAD~N" }`

**Final review prompt:**

```
The implementation is complete. Review the changes:
- Any bugs or edge cases missed?
- Does the implementation match what we agreed on during the debate?
- Code quality issues?
- Security concerns?

Be concise. Only flag issues worth fixing.
```

**Apply fixes** if the opponent identifies clearly valid concerns:

- Fix bugs and edge cases
- Commit each fix separately with clear messages

**Skip** minor style suggestions.

## Phase 7: Summary

Present a final summary to the user:

```
## Summary

**Implemented:** [One sentence describing what was built]

**Debate outcome (Claude vs OPPONENT):**
- Claude advocated: [key position]
- OPPONENT advocated: [key position]
- Final verdict: [who won which points, synthesized approach]

**Key decisions from debate:**
- [Decision 1 - who proposed it and why it won]
- [Decision 2 - who proposed it and why it won]

**Post-implementation fixes:**
- [Fix applied after final review, if any]

**Commits:**
- `abc1234` - [commit message]
- `def5678` - [commit message]
```
