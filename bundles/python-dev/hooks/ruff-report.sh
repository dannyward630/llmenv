#!/usr/bin/env bash
# ruff-report: report ruff findings for one edited Python file (#91).
#
# Report-only. Running `ruff check --fix` rewrote lines the edit did not touch
# and removed imports that the next edit still needed.
#
# Runs ruff from the file's git root, not the hook cwd. Ruff resolves `exclude`
# against the working folder, so a repo's own ruff.toml takes effect only there.
#
# Input: the PostToolUse JSON payload on stdin. Non-Python files pass silently.
set -euo pipefail

command -v jq >/dev/null 2>&1 || {
  echo "ruff-report hook: jq not found on PATH, so ruff findings are not reported. Install jq." >&2
  exit 1
}
f=$(jq -r '.tool_input.file_path // empty')
case "$f" in *.py) ;; *) exit 0 ;; esac

dir=$(dirname "$f")
root=$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null) || root="$dir"
# Ruff exits 1 when it finds issues. The report is the output, so the exit status is ignored.
out=$( (cd "$root" && ruff check --quiet "$f") 2>&1 | head -20 || true)
[ -n "$out" ] || exit 0
# Claude reads a PostToolUse hook only through additionalContext; plain stdout
# goes to the debug log.
jq -n --arg ctx "ruff findings in $f:
$out" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
