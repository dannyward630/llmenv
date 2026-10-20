#!/usr/bin/env bash
#
# check-arc-ownership.sh -- fail on MRC ownership attributes in ARC-compiled sources.
#
# Usage:
#   check-arc-ownership.sh [--root DIR] [--strict] [--verbose] [--help]
#
# Prints one finding per line on stdout: path:line:attribute
# Prints diagnostics on stderr.
# Exits 0 when clean, 1 when a finding is present, 2 on a usage error or when a
# project file or a source file cannot be read.
#
# The scan reads the -fno-objc-arc list from the project files, so a file that
# genuinely needs MRC does not raise a finding. The work is in
# arc_ownership_check.py beside this script.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYTHON_SCRIPT="${SCRIPT_DIR}/arc_ownership_check.py"

if [ ! -f "$PYTHON_SCRIPT" ]; then
  echo "check-arc-ownership: missing ${PYTHON_SCRIPT}" >&2
  exit 2
fi

if command -v uv >/dev/null 2>&1; then
  exec uv run --quiet --no-project python3 -I "$PYTHON_SCRIPT" "$@"
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "check-arc-ownership: 'python3' is not on PATH. Install Python 3 and try again." >&2
  exit 2
fi

exec python3 -I "$PYTHON_SCRIPT" "$@"
