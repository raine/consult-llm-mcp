---
name: implement
description: Autonomously plan and implement a task with external LLM review. Writes a behavioral spec, runs an evidence-gated plan review (premortem + independent alternative), applies feedback through a decision ledger, implements with a triggered debug loop, and finishes with an evidence-gated red-team pass. No user interaction.
allowed-tools: Bash, Glob, Grep, Read, Edit, Write
---

End-to-end autonomous workflow: spec → plan → review → implement → red-team → summary. Reviewers must produce structured findings with concrete evidence; conflicts resolve through a written decision ledger, not silent agent judgment.

**Load the `consult-llm` skill before proceeding** — it defines the invocation contract. Do not call the CLI without loading it first.

## Argument handling

**Arguments:** `$ARGUMENTS`

**Reviewer flags:** any `--<selector>` resolvable by `consult-llm models` selects a reviewer (e.g. `--gemini`, `--openai`). Repeat for multiple. With no selector flag, use **all** available selectors in parallel. Translate each into a `-m <selector>`.

**Mode flags:**

- `--consult-first` — gather independent reviewer proposals before writing the spec/plan, then synthesize into an Approach Decision Record. Use when scope is ambiguous, direction non-obvious, the change crosses module boundaries, or the repo is unfamiliar. Skip for typos, mechanical renames, exact bug fixes, or dependency bumps.
- `--rounds N` — repeat the review-refine cycle (Phases 3–4) up to N times. Default `1`, max `3`.
- `--no-review` — skip Phases 3, 4, and 6. Compatible with `--consult-first` (proposal generation still runs; plan review and red-team do not).

Strip all flags from arguments to get the task description.

## Phase 0: Snapshot state

Load `consult-llm`. Capture working-tree state for the Phase 6 diff base:

```bash
git rev-parse HEAD                    # store as START_HEAD
git status --short
git symbolic-ref --short HEAD
```

Halt conditions (do not auto-recover):

- Working tree shows changes outside expected files — user must clean or stash.
- No reviewer model resolves (unless `--no-review` is set and `--consult-first` is not).

## Phase 1: Gather context

Use Glob, Grep, Read to map the task surface. Make reasonable assumptions; do **not** ask the user clarifying questions.

Capture:

- Files, modules, public interfaces, dependents, migration concerns.
- Existing patterns and conventions.
- Validation contract: how do tests/types/lints run? Look in `CLAUDE.md`, `CLAUDE.local.md`, `justfile`, `Makefile`, `package.json`, `pyproject.toml`. Record the canonical command (e.g. `just check`).

Select a small set of source files most relevant to the change. These are passed as shared `-f <path>` to every reviewer call.

## Phase 2: Spec and plan

### 2A — Independent proposals (only if `--consult-first`)

Reviewers must see only the raw task and factual source context. Do **not** write inferred scope, assumptions, acceptance criteria, direction, or tasks before proposals are captured.

Save a context bundle to `history/<YYYY-MM-DD>-context-<topic>.md`. Include only:

- Raw task verbatim.
- Repo facts: branch, START_HEAD, validation command, test framework.
- Source inventory — files with **factual** reasons for inclusion only ("contains symbol X", "test file for Y"). No "likely needs change", no "probable approach".

Invoke `consult-llm` once with one `-m <selector>` per reviewer, `-f <context bundle>`, and `-f <relevant source>`. Capture `[thread_id:group_xxx]` from line 1 as `CONSULT_THREAD_ID` — it threads through 2A → 3 → 4 → 6. Synthesize the proposals directly into the ADR.

Prompt:

```
You are independently advising on how to implement the raw task using the attached source context.

You have NOT been given an agent-written spec, plan, architecture, or intended scope. Infer the most defensible scope and approach from the raw task and source evidence. Make assumptions explicit. Do not ask clarifying questions.

Output exactly these sections in this order:

## Scope Reading
- In scope:
- Out of scope:
- Assumptions:
- Ambiguities:
- Confidence: high | medium | low

## Recommended Approach
- Strategy (2-4 sentences):
- Files/modules likely touched:
- Implementation outline:
- Compatibility/migration impact:
- Complexity: low | medium | high

## Acceptance Criteria I Would Verify
Given/When/Then. Observable behavior only.

## Key Design Choices
For each: choice / rationale / tradeoff.

## Risks and Failure Modes
For each: risk / trigger / impact / mitigation_or_test.

## Alternative Worth Considering
- Strategy (materially different, not a minor variant):
- When it wins:
- Why not primary:

## Evidence To Check Before Planning
```

**Ambiguity / groupthink handling:**

- If two or more reviewers report `Confidence: low` and no proposal produces testable acceptance criteria, stop with an Ambiguity Blocker (record conflicting readings and required user decision in the ADR; do not implement).
- If proposals converge on one narrow strategy with no credible alternative on a high-risk or cross-module task, run one divergence-challenge consult on the same thread before synthesis.

Then write an Approach Decision Record at `history/<YYYY-MM-DD>-adr-<topic>.md`. Every proposal must be accepted, rejected, or watched-risk — no silent discard. Required sections:

- **Scope Divergence Matrix** — for each scope question: proposal readings / selected interpretation / rationale / risk.
- **Proposal Summary** — id / model / scope confidence / strategy / strengths / weaknesses / decision.
- **Selected Approach** — single coherent core architecture (data model, control flow, API boundary, ownership, persistence, concurrency must come from one proposal).
- **Frankenstein Guard** — if core choices mix across proposals, include an explicit compatibility proof. Otherwise the plan is invalid. Borrowing rejected proposals' tests, naming, validation, or error handling is fine.
- **Rejected Alternatives** — for each: reason / evidence / watched-risk?
- **Watched Risks** — risk / why accepted / what would change the call.
- **Evidence Checks Required Before/During Implementation**.

**Scope-divergence rule:** if divergence affects public API, data loss, security, or migration behavior and no reading is clearly supported, stop with an Ambiguity Blocker.

**Tiebreakers:** literal fit to raw task → safety/data integrity → acceptance criteria coverage → existing patterns → maintainability → testability → simplicity.

### 2B — Plan artifact

Save `history/<YYYY-MM-DD>-plan-<topic>.md`. With `--consult-first`, link the context bundle and ADR at the top, and reflect the ADR in spec/criteria/tasks.

````markdown
# <Feature> Plan

**Goal:** <one sentence>
**Approach:** <2-3 sentences>
**Assumptions:** <list>
**Validation command:** `<e.g. just check>`

## Behavioral Spec

- **In scope:**
- **Out of scope:**
- **Acceptance criteria** (Given/When/Then):
- **Invariants** (must always hold):

## Test Matrix

| # | Scenario | Expected behavior | Test file/command | Required before implementation? |
| - | -------- | ----------------- | ----------------- | ------------------------------- |

## Rollback

Required only when the change touches schema, on-disk format, or a public API contract. Rollback steps / data compatibility / rollback window.

## Tasks

### Task 1 — <short description>
- **Files:** create/modify with exact paths (and line ranges for modify)
- **Steps:** specific actions
- **Verifies acceptance criteria:** #1, #3
````

Guidelines: exact paths only, never "somewhere in src/". Each task small (2-5 minutes) and tied to acceptance criteria. DRY, YAGNI — only what the spec demands. Do not embed full implementation code in plan tasks; brief snippets only when the design choice is non-obvious.

## Phase 3: Plan review

Skip if `--no-review`. Reviewers receive the plan file and relevant source files; they must produce structured output.

Invoke `consult-llm` once with one `-m <selector>` per reviewer, `-f <plan path>`, `-f <relevant source>`. With `--consult-first`, continue from `CONSULT_THREAD_ID` via `-t <id>` and additionally attach `-f <context bundle>`, `-f <ADR>`.

Compose the prompt by including the `## ADR Check` section if and only if `--consult-first` was used; otherwise include `## Independent Alternative`. Send exactly one of these — do not include the bracket markers in the prompt sent to reviewers.

Capture the new `[thread_id:group_xxx]` for `--rounds` and Phase 6.

```
Review this implementation plan against the attached source context.

Output exactly these sections:

## Spec Check
List acceptance criteria that are missing, ambiguous, or untestable. Flag invariants the plan does not preserve. If sufficient, write "Spec sufficient."

## ADR Check (only when `--consult-first`)
- better_rejected_approach: <proposal id or "none">
- incompatible_merge_detected: yes | no
- selection_rationale_sufficient: yes | no
- required_change:

## Independent Alternative (only when not `--consult-first`)
In 3-5 sentences, the approach you would choose given the spec and source alone. Note any material divergence from the proposed plan.

## Premortem
Assume the plan ships and fails in production within six months. Top 3 failure modes. For each:
- failure_mode (concrete, with trigger):
- impact: low | medium | high
- probability: low | medium | high
- evidence (in plan or source):
- current_mitigation (quote plan or "none"):
- mitigation_sufficient: yes | no
- required_plan_change_or_test:

Only report failures with a concrete trigger and measurable impact.

## Plan Findings
For each issue:
- severity: must-fix | should-fix | optional
- issue_identity: <short kebab-case label>
- location_or_task: <plan section, task, or file:line>
- rationale:
- recommended_change:
```

## Phase 4: Verify findings, update ledger

**Verify before adopting.** Treat every reviewer finding (including must-fix) as an unverified claim. Reviewer severity is advisory.

Pick the cheapest method that proves or disproves the claim:

- **Plan claims** — re-read the cited plan section.
- **Source claims** — read the cited file against current code.
- **Library/API claims** — verify against library source or official docs. Use `gh search code` for usage patterns, `Grep` in `node_modules`/vendored deps, or a throwaway script in `/tmp/`.
- **Premortem claims** — confirm the trigger actually occurs in the planned design.

Classify each finding:

- **Confirmed and worth fixing** → adopt.
- **Confirmed but YAGNI** → real but trigger requires contrived inputs no caller produces, or fix is disproportionate. Record as Watched Risk.
- **Not a real issue** → reviewer misread plan/source. Record as rejected with disproof.

With `--consult-first`, findings claiming `better_rejected_approach` or `incompatible_merge_detected: yes` must be verified against the raw proposals and source before adoption.

Append a Feedback Ledger to the plan file:

```markdown
## Feedback Ledger — Round N

- **finding-id:** <short kebab-case>
  - **reviewer(s):** <models>
  - **severity:** must-fix | should-fix | optional
  - **decision:** accepted | rejected | watched-risk
  - **rationale & evidence:** <proof from codebase/docs>
  - **plan/spec/test change:** <action or "none">

## Watched Risks
- **<label>:** why accepted; what would change the call.

## Premortem Mitigations Applied
- <failure mode> → <plan change>
```

**Conflict tiebreakers** (in order): security on safety conflicts → spec coverage → existing patterns → simplicity.

Any premortem finding rated `mitigation_sufficient: no` AND (`probability: high` OR `impact: high`) **must** be addressed before Phase 5 — change the plan, add a test-matrix row, or record explicitly in Watched Risks.

**Multiple rounds (`--rounds N`):** for round 2+, reuse `-m` flags, pass `-t <group_thread_id>` and `-f <updated plan>`, and send: *"Revision N. Were previous concerns addressed? New issues introduced? Same four sections, focus on what changed."* Stop early if reviewers signal no further changes. Append a fresh ledger section per round.

## Phase 5: Implement

Implement tasks **in order**. The validation command must pass at the end.

1. **Spec-first per task** — write or extend the test that proves the linked acceptance criterion **before** implementation. Confirm it fails. Write the code. Confirm it passes.
2. **Plan drift halts.** If implementation requires touching files outside the plan or deviating from the agreed approach, **stop**. Update the plan with the deviation and a one-line reason, then continue.
3. **Validation** — run the validation command after every logical unit and again at the end. A task is **done** when (a) its tests pass, (b) its acceptance criteria are verified, and (c) validation is green for the touched scope.
4. **Auto-commit at end** — when all tasks are done and validation is green, create a single commit for all implementation changes. Lowercase imperative subject; body explaining the why per `CLAUDE.md`. Phase 6 fixes go in separate commits (see below).

### Triggered debug protocol

Activate only when **the same check fails twice**, OR a fix would require changing the plan/spec, OR the failure cause is unclear. Do not formalize debugging for ordinary fixes.

For each blocked attempt, append to a scratch section at the bottom of the plan file (do not commit):

```
- failing command:
- exact error:
- recent relevant changes:
- hypothesis:
- evidence-gathering command (read-only):
- result:
- conclusion:
- fix or plan revision:
```

Cap: **3 hypotheses**. If two have failed, consult reviewers with `--task debug`, the same selectors, and the latest `[thread_id:group_xxx]`:

```
We are blocked during Phase 5 implementation.

Task: <ref>
Failing command and full output:
<output>

Hypotheses already tried (and why they failed):
<list>

Relevant recent changes:
<diff or summary>

Give ranked hypotheses with concrete evidence checks. For each hypothesis, state the observation that would confirm or falsify it. Do not propose code changes until the falsification step is identified.
```

If the third hypothesis fails, stop. Record blocker, hypotheses, and unanswered evidence question in the Phase 7 summary. Do not loop.

## Phase 6: Red-team review

Skip if `--no-review`. Whether the diff is too narrow for adversarial review is the reviewer's call (it exits cleanly), not the agent's.

Re-list changed files against `START_HEAD`:

```bash
git diff --name-only --diff-filter=d <START_HEAD>
git ls-files --others --exclude-standard
```

If both empty, stop and report nothing implemented. Otherwise pass tracked files as `--diff-files <path>` and untracked as `-f <path>`. Skip binaries and lockfiles.

Invoke `consult-llm` with `--task review`, `-t <group_thread_id>` from Phase 4, `--diff-base <START_HEAD>`, the file flags above, and `-f <plan path>`.

```
Adversarially review this diff against the Behavioral Spec and Test Matrix in the plan file.

Use only attack lenses relevant to the changed surface:
- auth-bypass / authorization confusion
- injection / unsafe parsing
- race / concurrency / ordering
- data loss / migration / rollback
- fuzz / malformed or boundary input
- API contract / compatibility break
- spec violation / missing invariant enforcement

A finding counts as Verified only if it includes a concrete repro: failing input, curl command, race-window with timing, or a unit test that reproduces the bug. Speculation goes under "Unverified hypotheses" and may not be marked must-fix.

If the diff is too narrow for meaningful adversarial review (e.g. <20 lines, no auth/data/input surface), say so and exit cleanly.

Output exactly:

## Verified Findings

### Finding N
- severity: must-fix | should-fix
- persona: <attack lens>
- location: path:line
- spec_or_invariant_violated: <reference, or "none">
- repro_or_poc: <concrete reproduction>
- expected_failure:
- rationale:

## Unverified Hypotheses
- <bullet>

## Spec Coverage Gaps
- <criterion or invariant the diff does not satisfy>
```

### Verify each finding before fixing

The `repro_or_poc` is a claim — run it.

- **PoC reproduces, diff is responsible** → eligible to fix.
- **PoC reproduces, different code is responsible** → re-target the fix or note misattribution; do not silently fix the wrong place.
- **PoC does not reproduce** → drop. List under "rejected after verification" in the summary.
- **Reproduces only via inputs no caller produces, or impossible timing** → record as Watched Risk; do not fix.

### Apply fixes

Apply only `Verified Findings` that survived verification AND are `must-fix` AND localized (single hunk, single right answer, no interface changes). For each fix:

1. Re-read the file and apply the smallest correct change.
2. Run the validation command for the touched scope.
3. **Commit each fix separately** with a lowercase imperative subject and a body that names the failure mode prevented.

Do **not** auto-fix `should-fix`, `Unverified Hypotheses`, or `Spec Coverage Gaps`. List them in the Phase 7 summary.

If any `must-fix` cannot be fixed safely, stop and hand off — do not loop another review pass.

## Phase 7: Summary

Print:

```
## Summary

**Implemented:** <one sentence>
**Consult-first:** yes | no
**Context bundle:** history/<date>-context-<topic>.md | n/a
**ADR:** history/<date>-adr-<topic>.md | n/a
**Plan:** history/<date>-plan-<topic>.md
**Diff base:** <START_HEAD short sha>
**Review phases run:** Phase 3 [yes | skipped] · Phase 6 [yes | skipped]

**Plan review (Round N):**
- accepted: <count> (must-fix: X, should-fix: Y)
- rejected: <count>
- watched risks: <count>

**Implementation:**
- tasks completed: X / Y
- validation: passed | failed (<command>)
- plan deviations: <list, or "none">

**Red-team:**
- verified findings auto-fixed: <count>
- verified findings handed off: <list>
- unverified hypotheses: <count>
- spec coverage gaps: <list, or "none">

**Blockers (if any):**
- <description, hypotheses tried, evidence question outstanding>

**Commits:**
- <sha> — <subject>
```
