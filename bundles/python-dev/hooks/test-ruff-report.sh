#!/usr/bin/env bash
# Self-check for ruff-report.sh. Run from anywhere:
#   bash bundles/python-dev/hooks/test-ruff-report.sh
# shellcheck disable=SC2016  # test payloads are literal shell text
set -euo pipefail
cd "$(dirname "$0")"
hook="$PWD/ruff-report.sh"

fails=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
elsewhere="$tmp/session"
mkdir -p "$elsewhere"

payload() { # payload <file-path>
  python3 -c 'import json,sys; print(json.dumps({"tool_input": {"file_path": sys.argv[1]}}))' "$1"
}

report() { # report <file-path>: run the hook from a folder outside the repo
  payload "$1" | (cd "$elsewhere" && bash "$hook") 2>&1 || true
}

expect() { # expect <desc> <want-substring|""> <got>
  local desc="$1" want="$2" got="$3"
  if [ -z "$want" ] && [ -z "$got" ]; then
    echo "ok: $desc"
  elif [ -n "$want" ] && [[ $got == *"$want"* ]]; then
    echo "ok: $desc"
  else
    echo "FAIL: $desc (want '$want', got: $got)"
    fails=$((fails + 1))
  fi
}

# Repo whose ruff.toml excludes gen/ with force-exclude. Ruff only honors this
# when it runs from the repo root (#91).
repo="$tmp/repo"
mkdir -p "$repo/gen" "$repo/src"
git -C "$repo" init -q
printf 'force-exclude = true\nextend-exclude = ["gen"]\n' >"$repo/ruff.toml"
printf 'import os\n' >"$repo/gen/out.py"
printf 'import os\n' >"$repo/src/app.py"

expect "excluded file in a repo is not reported" "" "$(report "$repo/gen/out.py")"
expect "unused import in a repo file is reported" "F401" "$(report "$repo/src/app.py")"
# Claude reads a PostToolUse hook only through additionalContext.
expect "the report is PostToolUse additionalContext" "F401" \
  "$(report "$repo/src/app.py" | jq -r 'select(.hookSpecificOutput.hookEventName == "PostToolUse") | .hookSpecificOutput.additionalContext' 2>&1)"

# Report-only: the hook must leave the file as it was.
before=$(cat "$repo/src/app.py")
report "$repo/src/app.py" >/dev/null
expect "hook does not rewrite the edited file" "import os" "$(cat "$repo/src/app.py")"
[ "$before" = "$(cat "$repo/src/app.py")" ] || {
  echo "FAIL: hook changed $repo/src/app.py"
  fails=$((fails + 1))
}

# A file outside any git repo falls back to its own folder.
printf 'import sys\n' >"$tmp/loose.py"
expect "file outside a repo is still reported" "F401" "$(report "$tmp/loose.py")"

# Non-Python files pass silently.
printf 'import os\n' >"$tmp/notes.txt"
expect "non-Python file is ignored" "" "$(report "$tmp/notes.txt")"

if [ "$fails" -gt 0 ]; then
  echo "$fails check(s) FAILED"
  exit 1
fi
echo "all ruff-report checks passed"
