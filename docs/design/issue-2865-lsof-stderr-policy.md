# Issue #2865 — the lsof stderr policy for the state/tmp sweep

- **Issue:** https://github.com/phaedrus1992/llmenv/issues/2865
- **Milestone:** `v3.13.0`
- **Code:** `lsof_paths` in `src/materialize/open_files.rs`
- **Type:** decision record; no behavior change

## Problem

The `state/tmp` sweep (`llmenv clean` and `llmenv doctor --gc`) keeps every entry that a live process still holds open.
It builds that in-use list from `lsof -F n`.

`lsof` can exit 0 and still write to stderr.
It does that when it cannot stat a mount, and on some hosts that happens on every run.
An entry that is open on such a mount can then be missing from the in-use list, and the sweep can remove it.

A non-zero exit already stops the sweep.
This note covers exit 0 with stderr output only.

## Options

1. Log the stderr text as a warning and keep sweeping.
   A host with a routine warning keeps its sweep and its daily cleanup hint.
2. Stop the sweep on any stderr output.
   This is safe, but on a host that always prints the warning, the sweep and the hint never run again.
3. Stop only when a stderr line names a path under `state/tmp`, or the mount that holds it.
   This narrows option 2, but it depends on the lsof warning text, which is not a stable interface.

## Choice

Option 1, the behavior that shipped with #2814.

## Reason

The owner chose it on #2865.
A sweep that never runs on a host with a routine lsof warning lets `state/tmp` grow without bound, and that cost is certain.
The cost of option 1 needs an entry under `state/tmp` that is open on the mount lsof cannot stat, and that is past the retention age.
That mount must then also hold the state folder.

The warning stays in the log, so a removal that follows it can be traced.
A test (`an_lsof_that_writes_to_stderr_keeps_the_names_it_reported`) pins the behavior.
