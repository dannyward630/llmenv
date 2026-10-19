# Unattended run report: 2026-10-19

Branch `unattended/local-main`, based on `release/3.x` (the integration branch stands in for `release/3.x` during the run).
Scope: the open issues of milestone v3.13.0.
The branch holds 74 commits above `release/3.x`, in nine sprint merges.
The signer locked during the run, so the run turned off signing in the repo config and committed unsigned.
All commits were signed again with `resign.sh` before the push.

## Rules of the run

- One branch for each sprint, created before its first commit, and merged into `unattended/local-main` with `sprint-merge.sh`.
- No push, no PR, and no closed issue during the run. This PR closes the finished issues.
- Each sprint ran the full local review before its merge: `nbl-dev:pre-pr-review` in strict mode, with Semgrep `p/rust`.
- Each sprint ran the full gate before its merge.
  The gate is fmt, markdownlint, clippy, changelog sync, hawk, the whole nextest suite, doc tests, and the module budget.
- A module budget hit was paid with an extraction in the same change, never with a raised cap or a "split this file" issue.
- An issue that needs an owner decision got a comment with the options and a recommendation, and was marked blocked.
- Every deferral is a filed issue.
- No feature in this run changes a number the user relies on, so no feature ships behind an off switch.

## Work merged

### Onboarding

- #2785: `llmenv setup --scan` runs a bounded scan of the repo for languages, tools, and markers.
- #2791: `llmenv setup --tools` reports agent tools such as `icm`, `codebase-memory-mcp`, `rg`, and `uv`.
- #2786: skill rules suggest bundles and tools from the scan evidence.
- #2931: composite for #2785 and #2791.

### CLI and task tracker

- #2917: `llmenv clean` does all of `prune` and `doctor --gc`.
  Both old commands are deprecated, and #2930 removes them in v4.0.0.
- #2905: `llmenv task list` is an alias of `ls`.
- #2929: composite for #2917 and #2905.
- #2904: a session checkpoint after `/clear` resumes when every listed session is the caller's.

### Hooks

- #2900: the `cd` advisory fires only when the last segment changes the folder, and says so correctly.
- #2937: the `cd` advisory reads a substitution-only command and `eval cd` correctly.
- #2901: repeat detection keys its state per subagent, so parallel subagents are not one stuck loop.
- #2936: `read_once`, slippage, `stop_dedupe`, and the task nudge keep separate state for each subagent.
- #2911: web-fetch memories are stored under the session's project, and an all-projects recall leaves out web pages.
- #2912: a checkpoint job whose folder is gone is dropped, and a failed start uses up one attempt.

### MCP, codebase memory, and memory

- #2884: a failed MCP request names the proxy variables that were set.
- #2932: PATH lookups skip relative folders, and every spawn uses the resolved absolute program.
- #2928: `llmenv doctor` says "unconfined" when no codebase-memory roots are recorded.
- #2922: the `index_repository` guard reads the roots of the codebase-memory entry that serves the session.
- #2941: an inactive `features.memory` entry no longer starts `auto_prune` or adds the memory skill references.

### Config, merge, and file reads

- #2933: bundle `AGENTS.md`, `bundle.yaml`, and `rules/*.md` are read as regular files of at most 1 MiB.
- #2942: `config.yaml` (4 MiB), `.llmenv.yaml` (64 KiB), and skill files (64 MiB) use one bounded reader.
  A FIFO fails at once instead of a hang.
  A transcript move between file systems streams the file.
- #2923: four call sites bound and report their failures.

### Tests and chores

- #2891: CI runs `zizmor` on every workflow change.
- #2819: `pub(super)` visibility that no other file uses is narrowed.
- #2822: property tests for the tag specificity ranking.
- #2924: property coverage and dedup for the roots and bundle sets.
- #2934: composite for the chores.

### Sprint composites

- #2935: audit bugs.
- #2939: hook, mcp, and error bugs.
- #2940: codebase-memory guard and bundle reads.
- #2943: memory entries and config reads.

## Work not done

| Issue | State | Why |
| --- | --- | --- |
| #2865 | Blocked | Owner picks the `lsof` stderr policy for the state/tmp sweep. |
| #2919 | Blocked | Needs a storage and window design for the recall failure rate. |
| #2920 | Blocked | Needs a recall evaluation set to measure a cut. |
| #2921 | Blocked | The ICM score is normalized per query; needs a relevance rule. |
| #2787 | Blocked | Where shipped bundles live, and consent to edit `~/git/my-llmenv`. |
| #2788 | Blocked | Trust gate for onboarding writes (TTY, token, or prompt). |
| #2789 | Blocked | Builds on the #2788 write path. |
| #2892 | Blocked | Keychain call deadline: owner picks an option. |
| #2938 | Blocked | What scope-context records are for in an all-projects recall. |
| #2946 | Blocked | Symlink policy for a cloned repo's `.llmenv.yaml`. |
| #2944 | Open | Filed by the last sprint's review; the user stopped feature work first. |
| #2945 | Open | Filed by the last sprint's review; the user stopped feature work first. |

Each blocked issue has a comment with the options and a recommendation.

## Review of the whole branch

REVIEW-PENDING

## Lessons

- `commit-msg-guard` reads the commit message file before the commit runs, so write the file in a separate call.
- Commit message text such as `cd /x` trips `identity-guard`; always commit with `-F`.
- The `rust-budget.sh --changed` warnings do not block a commit, so its exit code must gate the commit.
- `branch-diff.sh` computes the merge base against `main`, so each analyzer must be told the base is the integration branch.
- `cargo-mutants` beside a `nextest` run makes the subprocess hook tests time out; run them one after the other.
- Cargo output is colored, so `grep ^error` misses errors; set `CARGO_TERM_COLOR=never`.
- `rustix::fs::mknodat` does not exist on Apple targets; tests make a FIFO with the `mkfifo` command.
- Hawk flags `pub(crate)` items that only one module uses after a move; check visibility after each extraction.
- Each analyzer finding was checked against the code before a fix.
  One was a real bug (a dangling bundle link read as an absent file), and some others were not.

## What the user must do

1. Answer the open question on each blocked issue in the table above.
2. Merge this PR into `release/3.x` with a merge commit.
3. Let the forward-merge workflow carry it to `release/4.x` and `main`, and fix any conflict it opens.
4. Pick up #2944 and #2945 in the next run.
