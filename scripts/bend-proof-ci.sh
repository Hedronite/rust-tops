#!/bin/sh
# Single CI entrypoint for bend-proof and bend-verdict.
# Refuses the env downgrades that bend-gate.sh allows for local names-only runs.

set -eu

if [ -n "${BEND_GATE_NAMES_ONLY:-}" ] || [ -n "${RUST_TOPS_LAWS:-}" ]; then
  printf 'bend-proof-ci: BEND_GATE_NAMES_ONLY/RUST_TOPS_LAWS must be unset in CI\n' >&2
  exit 1
fi

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

if [ "${1:-}" = "--verdict" ]; then
  if [ -n "${2:-}" ]; then
    printf 'bend-proof-ci: --verdict takes no other argument\n' >&2
    exit 2
  fi
  cd "$root/laws"
  exec bend PROOF.bend --verdict
fi

if [ -n "${1:-}" ]; then
  printf 'bend-proof-ci: unknown argument %s\n' "$1" >&2
  exit 2
fi

exec "$root/scripts/bend-gate.sh"
