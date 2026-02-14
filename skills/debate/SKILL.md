---
name: debate
description: LLMs propose and critique approaches, agent moderates the debate and synthesizes the best solution, then implements.
---

Have Gemini and Codex debate the best approach, then synthesize and implement.

## Configuration

**Arguments:** `$ARGUMENTS`

Check the arguments for flags:

**Mode flags:**
- `--dry-run` → debate and plan only, skip implementation
- `--skip-final` → skip the final review phase

Strip all flags from arguments to get the task description.

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

Have both LLMs propose their approach independently (in parallel).

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

Call BOTH simultaneously:

**Gemini** - `mcp__consult-llm__consult_llm` with:
- `model`: "gemini-3-pro-preview"
- `prompt`: Opening prompt above
- `files`: Array of relevant source files discovered in Phase 1

**Codex** - `mcp__consult-llm__consult_llm` with:
- `model`: "gpt-5.3-codex"
- `prompt`: Opening prompt above
- `files`: Array of relevant source files discovered in Phase 1

**Extract thread IDs:** Each response will include a `[thread_id:xxx]` prefix. Save both thread IDs (`gemini_thread_id`, `codex_thread_id`) for use in subsequent phases.

## Phase 3: Rebuttals

Have each LLM critique the other's approach (in parallel). Use `thread_id` to continue each LLM's conversation — they already have full context of the task and their own opening argument, so you only need to send the opponent's argument.

**Rebuttal prompt (same for both, just swap the opponent's argument):**
```
Your opponent proposed this alternative approach:
[Opponent's opening argument]

Provide a rebuttal:
1. **Critique**: What are the weaknesses in your opponent's approach?
2. **Defense**: Address any weaknesses in your own approach
3. **Concessions**: Are there any good ideas from your opponent worth adopting?
4. **Final position**: State your refined recommendation

Be constructive but thorough in your critique.
```

Call BOTH simultaneously:

**Gemini** - `mcp__consult-llm__consult_llm` with:
- `model`: "gemini-3-pro-preview"
- `prompt`: Rebuttal prompt with Codex's opening argument as the opponent
- `thread_id`: `gemini_thread_id` from Phase 2

**Codex** - `mcp__consult-llm__consult_llm` with:
- `model`: "gpt-5.3-codex"
- `prompt`: Rebuttal prompt with Gemini's opening argument as the opponent
- `thread_id`: `codex_thread_id` from Phase 2

## Phase 4: Moderator's Verdict

As the moderator, analyze the debate and synthesize the best approach:

1. **Score the arguments**:
   - Which approach is simpler?
   - Which approach better fits existing patterns?
   - Which critiques were valid?
   - What concessions were made?

2. **Identify consensus**: Where did both LLMs agree?

3. **Resolve disagreements**: For each point of contention:
   - Evaluate the arguments from both sides
   - Pick the stronger position or find a middle ground
   - Prefer simpler solutions when arguments are equally strong

4. **Write the verdict** as part of the plan:

````markdown
# [Feature Name] Implementation Plan

**Goal:** [One sentence describing what this builds]

## Debate Summary

**Gemini's position:** [1-2 sentence summary]
**Codex's position:** [1-2 sentence summary]

**Points of agreement:**
- [Consensus point 1]
- [Consensus point 2]

**Resolved disagreements:**
- [Issue]: Gemini said X, Codex said Y → **Verdict:** [Your decision and why]

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

Save the plan to `history/plan-<feature-name>.md`.

## Phase 5: Implement

**If `--dry-run`:** Skip to Phase 7 (Summary) - report the debate and plan without implementing.

Implement the plan without further interaction:

1. **Follow the plan exactly** - implement each task in order
2. **Commit after each logical unit** - keep commits small and focused
3. **If something is unclear** - make a reasonable decision and note it in the commit message
4. **If a task fails** - attempt to fix it before moving on
5. **Only stop if there's a blocking error** that cannot be resolved

Implementation rules:
- Work through tasks sequentially
- Test changes when possible
- Keep commits atomic and well-documented
- Use commit messages that explain the "why"

## Phase 6: Final Review

**If `--skip-final`:** Skip to Phase 7 (Summary).

After implementation, have both LLMs review the result (in parallel). Use `thread_id` to continue each LLM's conversation — they already have full context of the task and the debate, so you only need to send the review prompt and the diff.

**Final review prompt:**
```
The implementation is complete. Review the changes for bugs, issues, or improvements:
- Any obvious bugs or edge cases missed?
- Code quality issues (error handling, naming, structure)?
- Deviations from best practices?
- Security concerns?

Be concise. Only flag issues worth fixing.
```

Call BOTH simultaneously:

**Gemini** - `mcp__consult-llm__consult_llm` with:
- `model`: "gemini-3-pro-preview"
- `prompt`: Final review prompt
- `thread_id`: `gemini_thread_id` from Phase 2
- `git_diff`: `{ "files": [list of changed files], "base_ref": "HEAD~N" }`

**Codex** - `mcp__consult-llm__consult_llm` with:
- `model`: "gpt-5.3-codex"
- `prompt`: Final review prompt
- `thread_id`: `codex_thread_id` from Phase 2
- `git_diff`: `{ "files": [list of changed files], "base_ref": "HEAD~N" }`

**Apply fixes** if both reviewers identify the same issue, or if one raises a clearly valid concern:
- Fix bugs and edge cases
- Commit each fix separately with clear messages

**Skip** minor style suggestions or conflicting opinions.

## Phase 7: Summary

Present a final summary to the user:

```
## Summary

**Implemented:** [One sentence describing what was built]

**Debate outcome:**
- Gemini advocated: [key position]
- Codex advocated: [key position]
- Final verdict: [synthesized approach]

**Key decisions from debate:**
- [Decision 1 and why]
- [Decision 2 and why]

**Post-implementation fixes:**
- [Fix applied after final review, if any]

**Commits:**
- `abc1234` - [commit message]
- `def5678` - [commit message]
```
