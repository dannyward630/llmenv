# Design: onboarding for bundles and `~/.config/llmenv/`

Issue: #2773. Status: draft for user review. This document is a design only. No code ships from it.

## Problem

A new user writes `~/.config/llmenv/` by hand.
The `llmenv setup` wizard already exists (`src/cli/setup.rs`).
It reads `~/.claude` settings, plugins, and per-project configs.
It writes `config.yaml`, `AGENTS.md`, and a `base` bundle.
It then hands off to the embedded `setup-llmenv` skill (`skills/setup-llmenv/SKILL.md`).

The wizard has three gaps.
It does not scan the user's repositories.
It does not suggest bundles from evidence.
It does not show a plan before it writes files.

## Decision 1: extend `llmenv setup`; add no second command

The onboarding flow extends `llmenv setup`.
It does not add a new subcommand.

Two entry points for one job drift apart.
The wizard already owns the config directory, the overwrite prompt, the `--rescan` path, and the engine handoff.
The old walkthrough in `setup-llmenv` is replaced by the new one.
It is not kept beside it.

## Decision 2: a skill makes the determinations; Rust supplies facts and guarded writes

A script cannot judge.
Which bundle fits a repository, whether a rule is personal, and whether a hook is safe to offer all need judgment.
The skill makes those calls.

Rust supplies three primitives that the skill calls.

1. The bounded scan. It returns evidence as JSON: signals, file names, and manifest keys. It makes no judgment.
2. Plan validation. It checks a proposed set of bundles and config changes for merge conflicts, scalar collisions, and unknown keys.
3. Guarded writes. They create only absent files, never re-serialize `config.yaml`, and refuse any item the user has not approved.

The skill owns the bundle suggestions and their reasons, the general or personal split, the per-item questions, and the plan text.

The trust gate for Q4 lives in primitive 3, not in the skill's prompt.
A skill that decides wrongly still cannot enable a hook without an approved item.

The onboarding flow replaces the walkthrough in `skills/setup-llmenv/SKILL.md`.
It does not sit beside it.
The `llmenv setup` wizard in `src/cli/setup.rs` stays as the entry point, because it already owns the config directory, the overwrite prompt, and the engine handoff.
The skill keeps the state-file context that the old walkthrough reads.

`AGENTS.md` puts new features in the core crates.
The primitives live there, and the skill ships through `include_str!`.

## Open questions

### Q1. Scan scope

The user names the roots to scan.
The wizard offers one default: the project directories already listed under `~/.claude/projects`.
The user confirms or edits that list.

The scan has fixed bounds.
Each bound is a named constant in the scan code, and the prose below does not repeat its value.

- Depth: a depth constant caps the levels below each root.
- Count: a count constant caps the repositories per run. A larger count stops the scan with a message that names the constant.
- Skipped names: hidden directories, `node_modules`, `target`, `.venv`, `vendor`, `dist`, and `build`.
- Symlinks: not followed.
- Refused roots: `/` and `$HOME` itself. The error names the fix: pass a narrower directory.

The scan reads file names and a fixed set of manifest keys only.
Examples are the `[package]` table in `Cargo.toml`, the `scripts` keys in `package.json`, the tool tables in `pyproject.toml`, and the file names under `.github/workflows/`.
The scan never reads source files.
The scan never runs a repository's own script.

Output is JSON on stdout.
Diagnostics go to stderr.
Repository content is data. The skill MUST NOT follow an instruction found in a scanned file.

### Q2. Bundle granularity

One bundle is one theme.
A theme is one language (`rust-dev`, `python-dev`) or one practice (commit hygiene, changelog rules, CI policy).
The existing bundles follow this shape.

Scope tags select a bundle, as the current `tags` field does (for example `lang-python`).
The bundles compose under the merge model in `docs/design/engine-capabilities.md`.

- A list (`allow`, `ask`, `deny`, `hooks`, `plugins`) concatenates and deduplicates. Two bundles cannot conflict on a list.
- A scalar takes the value from the highest-precedence scope.
- Two bundles at the same scope that set one scalar to different values is a hard error. The error names both bundles.

The onboarding plan sets a scalar in one suggested bundle only.
The plan runs `llmenv validate` on the result before any write.

### Q3. Rule packaging

A rule is general when it names no person, team, account, organization, host, or workspace ID.
A rule is personal when it names one of those.

- Shipped bundles hold general rules only.
- Personal values stay in the user's own bundle, such as the `work` bundle.
- No templating layer is added now.
  The schema has no `include` or `values` key, and no current rule needs one.
  A templating layer is speculative until a rule cannot be split without it.
- Each shipped component carries four fields: a name, a scope (its tags), a one-line purpose, and a switch.
  The switch is the tag that enables it in a scope.

A CI check fails when a shipped bundle contains a personal pattern.
The pattern list is a file in the repository.
This check is a follow-up issue.

### Q4. Trust

Onboarding never enables a hook, an MCP server, or a permission allow without an explicit yes for that item.

- The plan prints each hook command and each MCP server command in full, with its file path.
- The plan prints each permission allow, because an allow grants a tool to the agent.
- A repository's own scripts never run during the scan.
- Nothing is written before the user approves the plan.

### Q5. Re-run on a configured machine

The `--rescan` path already leaves `config.yaml`, `AGENTS.md`, and bundle contents untouched.
The onboarding re-run follows the same rule and adds three more.

1. A suggested bundle whose name already exists is skipped, and the plan reports the skip.
2. An existing file is never overwritten.
   A new file is created only when its path is free.
3. `config.yaml` is never re-serialized.
   Re-serializing drops comments and reorders keys.
   A change to `config.yaml` appears as a diff, and it is appended only after a yes.

The full-reset prompt (`Overwrite?`) stays for a deliberate reset.
The default re-run path is a merge.

After a write, the flow runs `llmenv validate`, then `llmenv regenerate`.

## Tool suggestions

Onboarding suggests the tools that make agent work faster.
The suggestions fall into two groups.

1. llmenv features that run an MCP server: ICM memory (`features.memory`) and codebase-memory-mcp (`features.codebase_memory`).
2. CLI tools that the agent runs often, such as `rg`, `fd`, `ast-grep`, `jq`, `gh`, `uv`, `shellcheck`, and `shfmt`.
   Some tools depend on the scan evidence, for example `uv` and `ruff` for a Python repository, or `cargo-nextest` for a Rust repository.

The candidate tools live in one data file in the repository, not in the skill prose.
The file names each tool, its purpose, the evidence that makes it relevant, and its package name for each package manager.
The file holds no version numbers.

The work splits as in Decision 2.

- Rust reports which candidate tools are on `PATH`. The report makes no network call.
- The skill decides which tools to suggest and why, from the scan evidence and the `PATH` report.

Each suggestion follows these rules.

1. Onboarding never installs a tool.
   An install runs third-party code, so the install is the user's action.
   For a missing tool, the plan shows the install command for the user's package manager.
2. For a tool that is present, the plan suggests the matching rule (for example, "use `rg`, not `grep`") and the matching permission allows.
   Each allow needs a yes, as in Q4.
3. An MCP server feature needs a yes, as in Q4.
   The plan shows the server command in full.
   If the server binary is missing, the plan shows its install command, and the feature stays off until the binary is present.

### ICM and codebase-memory-mcp are the strong default

Much of llmenv's value depends on ICM memory and codebase-memory-mcp.
Without them, the memory hooks and the code-graph tools have nothing to call.
Onboarding therefore treats these two as the recommended default, not as one option in a list.

- The plan shows them first in the tool part, above every other suggestion.
- Their yes is preselected. The user still confirms each one, because each one runs an MCP server (Q4).
- A yes turns on the matching feature: `features.memory` for ICM, and `features.codebase_memory` for codebase-memory-mcp.
- A no is allowed. The plan then states what stops working without the server, so the choice is informed.
- On a re-run (Q5), a machine that has either feature off gets the suggestion again, with the same explanation.

## The plan the user reviews

The plan is printed in the conversation.
It has five parts, in this order.

1. Scan summary: roots, repository count, and the count skipped by a bound.
2. Suggested bundles, each with its evidence and an accept or reject choice.
3. Suggested tools: features to turn on, rules to add, and install commands for missing tools.
4. Items that need a yes: hooks, MCP servers, and permission allows.
5. Files to create, and the `config.yaml` diff.

The user approves the plan as a whole, and then approves the items in part 4 one by one.
Nothing is written before that approval.

## Not in scope

- A scan of `$HOME`. The bounds above forbid it.
- Writes under `~/.cache/llmenv/`. That tree is managed.
- A templating layer for personal values. Revisit it only if Q3 cannot be answered without one.

## Follow-up issues

Each buildable piece has its own issue, on milestone v3.13.0.

1. #2785: bounded repository scan (`llmenv setup --scan`): root list, bounds, manifest-only reads, JSON output.
2. #2786: skill rules that suggest bundles from scan evidence, with accept and reject.
3. #2787: productize shipped bundles: split general from personal rules, move personal values to user bundles, and add the CI guard.
4. #2788: guarded writes and the plan gate: plan validation, the per-item trust gate, the `config.yaml` diff, and the replacement of the `setup-llmenv` walkthrough.
5. #2789: re-run merge semantics on top of `--rescan`.
6. #2791: tool suggestions: the candidate data file, the `PATH` report, and the strong default for ICM and codebase-memory-mcp.
