#!/usr/bin/env bash
# Print the CI cache key for the cargo-hawk binaries.
#
# Usage: scripts/hawk-cache-key.sh [--help]
#
# cargo-hawk links rustc internals, so a binary works only with the exact rustc
# that built it. The key holds the latest cargo-hawk release and the rustc commit
# hash, so a new release of either one misses the cache (#1584, #2774).
#
# Stdout: "key=<key>" and "version=<cargo-hawk version>", one per line, in the
# GITHUB_OUTPUT format. Diagnostics go to stderr.
set -euo pipefail

if [[ "${1:-}" == "--help" ]]; then
    sed -n '2,11p' "$0"
    exit 0
fi
if [[ $# -gt 0 ]]; then
    echo "hawk-cache-key: unknown argument '$1'. Run with --help." >&2
    exit 2
fi

search=$(cargo search cargo-hawk --limit 1 --color never)
search_re='^cargo-hawk = "([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?)"'
if [[ ! "$search" =~ $search_re ]]; then
    echo "hawk-cache-key: cannot read the cargo-hawk version from 'cargo search' (got '${search:0:200}'). Check access to crates.io." >&2
    exit 1
fi
version=${BASH_REMATCH[1]}

commit=$(rustc -vV | sed -n 's/^commit-hash: //p')
if [[ ! "$commit" =~ ^[0-9a-f]{40}$ ]]; then
    echo "hawk-cache-key: cannot read the rustc commit hash from 'rustc -vV' (got '${commit}'). Check the active toolchain." >&2
    exit 1
fi

echo "key=cargo-hawk-${version}-rustc-${commit}-${RUNNER_OS:-local}"
echo "version=${version}"
