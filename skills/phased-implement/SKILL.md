---
name: phased-implement
description: Coordinator skill that runs a multi-phase implementation across workmux worktrees. Each phase invokes /implement in its own worktree; the coordinator handles dispatch ordering (sequential, parallel, DAG), merge sequencing, and failure isolation. Composes /implement, /merge, workmux, and consult-llm.
allowed-tools: Bash, Read, Write, Glob, Grep
disable-model-invocation: true
---

# Phased Implement

You are a coordinator. You **never** edit source files yourself. You write a master plan, spawn `workmux` worktree agents that each run `/implement` for one phase, and merge their work back via `/merge` in deterministic topological order.

**Load the `consult-llm` skill before any CLI call.** Follow its invocation contract (quoted heredoc, `__CONSULT_LLM_END__` terminator, `timeout: 600000`, thread-id capture). The `coordinator` skill (workmux) is also a useful reference for spawn/wait/capture/send patterns.

## Argument handling

**Arguments:** `$ARGUMENTS`

**Modes:**

- Free-form task — the coordinator generates the master plan via `consult-llm --task plan`.
- `--plan <path>` — skip generation; load an existing phased plan from `<path>`.

**Coordinator-level flags (apply ONLY to master-plan generation, not forwarded to per-phase `/implement`):**

- `--consult-first` — gather independent reviewer proposals before drafting the master plan (mirrors `implement` skill's Phase 2A).
- `--<selector>` (e.g. `--gemini`, `--openai`) — reviewer selection, repeatable. Default: all available selectors from `consult-llm models`.

**Per-phase flags** — declared in the plan's YAML `implement_flags:` list. The coordinator never injects flags into a phase.

**Other flags:**

- `--integration-branch <name>` — branch to merge into. Default: current branch at coordinator startup.

Strip flags from arguments to get the task description.

## Operating principles (read first)

- **Coordinator never writes source.** All implementation happens inside spawned worktree agents via `/implement`.
- **`workmux done` ≠ success.** A phase is successful only when its agent has written a result sentinel reporting `status=success`. See "Phase result sentinel" below.
- **Merges are serialized.** At most one `/merge` in flight globally.
- **Drain before dispatch.** `wait --any` returns on the first transition only. Before spawning, re-check `workmux status` and merge every handle in `done` — siblings that finished in the same window must not be left for the next wait.
- **`/merge` is invoked with `--keep`** so the coordinator can verify success before destroying the worktree. Coordinator runs `workmux remove <handle>` after verification.
- **Dependents only spawn when all predecessors are `merged`.** No exceptions.
- **No tight polling.** All waits use `workmux wait` with explicit timeouts.
- **Failure halts dependents.** No auto-retry. Failed/blocked worktrees are preserved for inspection.
- **YAML reasoning is native.** The coordinator (you, the LLM) reads `plan.md`'s YAML block semantically. Do **not** write `awk`/`grep` parsers for it. Use Bash only for `git`, `workmux`, `consult-llm`.

## Phase 0 — Snapshot and control plane

```bash
START_HEAD=$(git rev-parse HEAD)
# Honor --integration-branch <name> if provided; otherwise use current branch.
INTEGRATION_BRANCH="${INTEGRATION_BRANCH_FLAG:-$(git symbolic-ref --short HEAD)}"
# Verify the branch exists.
git rev-parse --verify "$INTEGRATION_BRANCH" >/dev/null
git status --short
```

**Halt conditions:**

- Working tree shows uncommitted changes outside `history/` — user must clean or stash.
- `INTEGRATION_BRANCH` resolves to a detached HEAD or does not exist — abort.
- No `--plan` was provided AND `consult-llm models` resolves no selectors — abort.

Pick a topic slug from the task description. Create the control-plane directory:

```bash
TODAY=$(date +%F)
PLAN_DIR="history/${TODAY}-phased-${TOPIC}"
mkdir -p "$PLAN_DIR/prompts" "$PLAN_DIR/captures"
```

Track phase state in your own working memory. Status values: `pending | working | done-unverified | merging | merged | failed | blocked`.

## Phase 1 — Master plan

### 1.a — Load or generate

If `--plan <path>` was provided: copy the file to `$PLAN_DIR/plan.md`. Skip 1.b.

Otherwise generate via `consult-llm`. With `--consult-first`, mirror `implement` Phase 2A: write a context bundle, consult all selectors with `--task plan`, capture the group `thread_id`, write proposals + ADR, then synthesize. Without `--consult-first`, do a single `consult-llm --task plan` call.

The generated plan must be a markdown file with a fenced YAML block defining the phase DAG (schema below). Write it to `$PLAN_DIR/plan.md`.

### 1.b — Plan review

Review must not reuse the planner's thread. Drop `-t`, pass the plan as `-f`, run multiple selectors in parallel: `consult-llm --task review -m <r1> -m <r2> -f $PLAN_DIR/plan.md`.

Use the review structure from `implement` Phase 3 (Spec Check / Premortem / Plan Findings). Apply must-fix changes, append a Feedback Ledger to `plan.md`. Single round only.

### 1.c — Plan visibility across worktrees

`history/` is gitignored and shared across worktrees (symlinked into each new worktree by the workmux setup). The plan, prompts, captures, and result sentinels all live there and are visible to spawned worktree agents directly via the same path. **Do not commit any of these files.** Coordinator and phase agents read/write the shared `history/<plan-dir>/` tree directly.

### Plan schema

`plan.md` must contain exactly one fenced YAML block of this shape near the top:

```yaml
phases:
  - id: <kebab-case-id>          # required, unique
    description: <one sentence>  # required
    depends_on: []               # required, list of phase ids (may be empty)
    paths:                       # optional, file-scope hint passed to /implement
      - "src/foo/**"
    acceptance:                  # optional, excerpt copied verbatim into prompt
      - "Given X, when Y, then Z"
    implement_flags:             # optional, default: []
      - "--no-review"
```

**Hard constraints (the LLM coordinator must verify on read):**

- Every `depends_on` entry exists in the `phases` list.
- No cycles.
- Every `id` is unique and matches `[a-z0-9-]+`.
- `implement_flags` strings are passed verbatim to `/implement`. Do not inject `--consult-first` automatically; the master plan already absorbed planning cost.

If the YAML block is missing or malformed, halt with a clear error and ask the user to fix `plan.md`. Do **not** silently regenerate.

### Worked example (3-phase diamond)

```yaml
phases:
  - id: api-contract
    description: Define and lock the new public API surface for X.
    depends_on: []
    paths: ["src/api/**", "tests/api/**"]
  - id: server-impl
    description: Implement the server side of the new API.
    depends_on: ["api-contract"]
    paths: ["src/server/**", "tests/server/**"]
  - id: client-impl
    description: Implement the client side of the new API.
    depends_on: ["api-contract"]
    paths: ["src/client/**", "tests/client/**"]
  - id: integration-test
    description: End-to-end tests crossing client and server.
    depends_on: ["server-impl", "client-impl"]
    paths: ["tests/integration/**"]
```

`api-contract` runs first. Then `server-impl` and `client-impl` run in parallel. After both merge, `integration-test` runs.

## Phase 2 — Dispatch loop

Hold the DAG and per-phase status in your own working memory. Use `workmux status` whenever you need a live view of running handles.

**Loop:**

1. Compute the **ready set**: phases whose status is `pending` and whose every `depends_on` entry has status `merged`.
2. If ready set is empty AND nothing is `working`/`merging`: all done → Phase 4 (summary).
3. For each phase in the ready set, write a prompt file (next section) and spawn:

   ```bash
   workmux add "<phase-id>" -b --base "$INTEGRATION_BRANCH" -P "$PLAN_DIR/prompts/<phase-id>.md"
   ```

   Track the phase as `working` with its handle.
4. Confirm the spawn started:

   ```bash
   workmux wait <handles...> --status working --timeout 120
   ```

   On exit-code 1 (timeout), inspect `workmux status` for each handle: a fast-completing phase may have skipped through `working` straight to `done`. Treat `done` as a valid post-spawn state and proceed to step 5; only mark `failed` if a handle is missing entirely or in `waiting`. On exit-code 3, mark the missing handle `failed` and proceed to halt logic.
5. **Recompute the live handle set** before every wait: only handles you are currently tracking as `working` belong in the wait list. Exclude handles already moved to `done-unverified`, `merging`, `merged`, or `failed` — otherwise `wait --any` returns immediately on a handle's already-finished state and the loop spins.

   Wait for the next transition with bounded chunks. Use 5-minute (300s) chunks; on each timeout, inspect statuses to detect `waiting`:

   ```bash
   workmux wait <all-working-handles> --any --timeout 300
   rc=$?
   if [ $rc -eq 0 ]; then
     # wait --any returns on the FIRST transition; multiple may be done.
     workmux status <all-working-handles>
   elif [ $rc -eq 1 ]; then
     # Timeout — inspect for stuck waiting agents.
     workmux status <all-working-handles>
     # If any handle is in `waiting` status, mark it `blocked` and halt dependents.
     # Otherwise loop back to Step 5.
   elif [ $rc -eq 3 ]; then
     # An agent exited unexpectedly. Mark the missing handle `failed`.
   fi
   ```

   Treat `done` as **unverified success**: set status to `done-unverified` and proceed to phase verification (Phase 3).
6. **Drain every `done` handle before spawning.** Run Phase 3 verification + merge on each (serial — merges are globally serialized). Only after the working set has no `done` handles do you recompute the ready set and loop to step 1. Skipping the drain spawns dependents against a stale `INTEGRATION_BRANCH`.

**Halt logic.** When any phase becomes `failed` or `blocked`:

- Stop spawning new phases.
- Mark every transitive dependent of the failed phase as `blocked` with `notes: blocked-by:<id>`.
- Allow currently `working` phases that are NOT transitive dependents to finish; verify and merge them normally.
- After the loop drains, jump to Phase 4 with a non-zero summary.

## Phase result sentinel

Each phase prompt instructs the agent to write, as the **final action of `/implement`**, a one-line result file:

```
<repo-root>/<plan-dir>/captures/<phase-id>.result
```

Format (single line, no surrounding whitespace):

```
PHASE_RESULT id=<phase-id> status=success|failed commit=<sha> validation=passed|failed
```

The agent writes `status=success` only if `/implement` Phase 7 reports validation passed and no blockers. Otherwise `status=failed`. The `commit` is the head of the worktree's branch (`git rev-parse HEAD` inside the worktree).

`history/` is shared across worktrees, so the coordinator reads the sentinel directly from `$PLAN_DIR/captures/<phase-id>.result` after the handle reaches `done`. No commit needed.

## Per-phase prompt template

Write this to `$PLAN_DIR/prompts/<phase-id>.md` before each spawn. **Self-contained**: assume the agent cannot see your context. Use **relative paths** (each worktree has its own root).

```markdown
# Phased Implement — phase <phase-id>

You are an isolated worktree agent. The coordinator spawned you to implement
**only** this phase of a larger phased plan. Do not work on other phases. Do
not modify the master plan.

## Phase

- **id:** <phase-id>
- **description:** <description>
- **depends_on:** <comma-separated, or "none">
- **file scope (advisory):** <paths joined>
- **acceptance criteria:**
  <verbatim excerpt from plan>

## Master plan

The master plan lives at `<plan-dir>/plan.md`. The `history/` directory is
gitignored and shared across worktrees, so this path is visible from your
worktree even though it is not part of your branch. Read it for cross-phase
context but do **not** modify it.

## What to do

1. Run `/implement <implement_flags...> <description>` to plan and implement
   this phase. The /implement skill will write its own per-phase plan,
   review it, implement it, run a red-team pass, and commit on success.
2. **Do not initiate `/merge` yourself.** After your work is committed and
   the sentinel is written (step 3), wait. The coordinator will send you
   `/merge --keep` as an explicit instruction once it has verified your
   sentinel. When that command arrives in this session, run it — it is the
   coordinator-managed merge step, not a user override. Do not ask for
   confirmation; just run it.
3. As the very last step, write the phase result sentinel to the shared
   `history/` tree (do NOT commit it — `history/` is gitignored):

   ```bash
   COMMIT=$(git rev-parse HEAD)
   STATUS=success     # or "failed" if /implement reported blockers
   VALIDATION=passed  # or "failed" if validation failed
   mkdir -p <plan-dir>/captures
   echo "PHASE_RESULT id=<phase-id> status=$STATUS commit=$COMMIT validation=$VALIDATION" \
     > <plan-dir>/captures/<phase-id>.result
   ```

## Constraints

- Stay within the file scope above unless /implement's plan-drift halt is
  triggered (in which case follow /implement's protocol).
- Do not run `git push`, do not modify other phases' files.
- If you cannot complete the phase, write `status=failed` in the sentinel and
  exit. The coordinator will halt dependent phases.
```

## Phase 3 — Per-phase verification and merge

When a handle transitions to `done`:

1. **Read the sentinel** directly from the shared `history/` tree:

   ```bash
   cat "$PLAN_DIR/captures/<phase-id>.result"
   ```

   If the file is missing or `status=failed` or `validation=failed`: mark phase `failed`, capture `workmux capture <handle> -n 200` to `$PLAN_DIR/captures/<phase-id>.tail`, halt logic.

2. **Capture the tail** for the audit trail regardless:

   ```bash
   workmux capture <handle> -n 200 > "$PLAN_DIR/captures/<phase-id>.tail"
   ```

3. **Serialize merges.** If another phase is currently `merging`, wait for that one to finish first.

4. **Trigger merge with `--keep`.** Send the `/merge` skill with `--keep` so the worktree survives for verification. Use chunked waits so a `/merge` that drops into a `waiting` prompt on a rebase conflict is detected fast instead of hanging the full timeout:

   ```bash
   workmux send <handle> "/merge --keep"
   # Chunked wait: at most 600s total, 60s chunks, abort early on `waiting`.
   elapsed=0
   while [ $elapsed -lt 600 ]; do
     workmux wait <handle> --timeout 60
     rc=$?
     if [ $rc -eq 0 ]; then break; fi
     # On timeout, peek status — `waiting` means /merge stalled on a conflict.
     if workmux status <handle> | grep -q waiting; then rc=2; break; fi
     elapsed=$((elapsed + 60))
   done
   # rc: 0 done, 1 timeout (overall), 2 stuck waiting, 3 agent exited.
   ```

   On `rc != 0`: mark `failed`, capture tail to `<plan-dir>/captures/<phase-id>.merge.tail`, halt.

5. **Verify ancestry against the worktree's POST-rebase tip.** `/merge` runs `git rebase <base>` before merging, which **rewrites commit SHAs**. The pre-rebase sha in the result sentinel is therefore not a stable ancestry token. Re-read the worktree's branch tip after `/merge` returns:

   ```bash
   # The worktree still exists because we used --keep.
   POST_TIP=$(workmux run <handle> -- git rev-parse HEAD | tail -1)
   if git merge-base --is-ancestor "$POST_TIP" "$INTEGRATION_BRANCH"; then
     # Merged successfully. Track the phase as merged with commit=$POST_TIP.
     workmux remove <handle>
     # Track phase as merged with commit=$POST_TIP.
   else
     # Merge silently failed or was skipped.
     # Mark phase failed; halt dependents. Do NOT remove the worktree.
   fi
   ```

   The sentinel's pre-rebase sha is **never** the ancestry-check input.

6. **Drift check.** If `INTEGRATION_BRANCH` advanced by commits not authored by phased-implement (external commits during the run), note it and continue. If a parallel sibling has produced conflicts, the next sibling's `/merge --keep` will surface them; treat that as a normal failure.

## Phase 4 — Summary

After the dispatch loop drains (success or halt), print:

```
## phased-implement summary

**Plan:** <plan-dir>/plan.md
**Integration branch:** <name>
**Start HEAD:** <short-sha>
**End HEAD:** <short-sha>

| id | status | commit | notes |
| -- | ------ | ------ | ----- |
| <each phase> |

**Merged:** <count> / <total>
**Failed:** <list of ids>
**Blocked:** <list of ids with blocked-by>

**Artifacts:**
- prompts: <plan-dir>/prompts/
- captures: <plan-dir>/captures/

**Next steps (if any failed/blocked):**
- Inspect captures/<id>.tail and captures/<id>.result.
- Worktrees for failed/blocked phases are preserved (use `workmux list`).
- To retry: no automatic resume. Manually `workmux remove` leftover failed worktrees, prune already-merged phases from `plan.md`, and re-invoke `/phased-implement --plan <plan-dir>/plan.md` on the trimmed plan.
```

## Watched Risks (documented limitations)

- **No `--continue` resume.** If the coordinator session dies mid-run, worktrees keep running. The user can inspect `workmux status` and re-invoke manually on a trimmed plan; v1 does not auto-reconcile partial state.
- **Plan drift cross-phase.** If a later phase discovers an earlier merged phase chose the wrong abstraction, the coordinator does not auto-replan. Halt and let the user re-plan.
- **paths-scope leakage.** `paths:` is advisory. The coordinator does not run `git diff --name-only` against allowed paths in v1; relies on `/implement`'s plan-drift halt and red-team.
- **Base-branch drift.** External commits on the integration branch during a run are tolerated but warned. Heavy drift can cause cascading rebase conflicts.
- **History/ visibility assumption.** This skill assumes `history/` is gitignored and shared (symlinked) across worktrees by the workmux setup. If that is not true in a given repo, plan/sentinel/captures will not be visible to spawned agents. Verify with `workmux` configuration before running.

## Invocation contract recap

- Coordinator-only skill. `disable-model-invocation: true` in frontmatter — invoke explicitly via `/phased-implement`.
- Reads YAML semantically; never parses with shell tools.
- Spawns via `workmux add`, communicates via `workmux send`, awaits via `workmux wait`, cleans via `workmux remove`.
- Generates plans via `consult-llm` (load that skill first).
- Trusts `/implement` for actual implementation and `/merge --keep` for branch integration; verifies both via the result sentinel (success/fail signal) and a post-rebase ancestry check using the worktree's branch tip read **after** `/merge` returns.
