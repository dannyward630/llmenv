#!/usr/bin/env bash
# Checks .github/renovate.json5 with a pinned Renovate config validator.
#
# An invalid option stops Renovate before it opens any PR, and it does so
# without a CI failure (#3114). This test fails the build instead. It also
# proves the validator rejects a known-bad option, so a silent pass is caught.
#
# Usage: bash .github/workflows/__tests__/renovate-config.sh
set -euo pipefail

# Exact version: a floating range would let the validator change under CI.
# Chosen only after the 24-hour publish delay (see #3114).
readonly RENOVATE_VERSION="44.195.1"
readonly CONFIG=".github/renovate.json5"
readonly FIXTURE_DIR=".tmp/renovate-config-test"
readonly FIXTURE="${FIXTURE_DIR}/bad-post-update-option.json5"

# Scripts are off so no postinstall code runs. The validator then uses its
# JavaScript regex fallback, which is enough for this config.
validator() {
  NPM_CONFIG_IGNORE_SCRIPTS=true npx --yes --package "renovate@${RENOVATE_VERSION}" \
    renovate-config-validator "$@"
}

mkdir -p "$FIXTURE_DIR"
cat >"$FIXTURE" <<'EOF'
{
  "postUpdateOptions": ["cargoLockPackages"],
}
EOF

if ! validator "$CONFIG"; then
  echo "FAIL: ${CONFIG} does not validate against renovate@${RENOVATE_VERSION}." >&2
  echo "Fix: remove the option the validator names, then rerun this test." >&2
  exit 1
fi

bad_out="$(validator "$FIXTURE" 2>&1)" && bad_rc=0 || bad_rc=$?
if [[ "$bad_rc" -eq 0 ]]; then
  echo "FAIL: the validator accepted ${FIXTURE}, which holds a known-invalid option." >&2
  echo "Fix: check that renovate@${RENOVATE_VERSION} still validates postUpdateOptions." >&2
  exit 1
fi
if [[ "$bad_out" != *cargoLockPackages* ]]; then
  echo "FAIL: the validator rejected ${FIXTURE}, but its output does not name the option." >&2
  echo "Fix: read the validator output above for the reason and update this check." >&2
  exit 1
fi

echo "OK: ${CONFIG} validates, and the validator rejects the known-bad option."
