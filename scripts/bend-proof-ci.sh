#!/bin/sh
# Single CI entrypoint for bend-proof and bend-verdict.
# Refuses the env downgrades that bend-gate.sh allows for local names-only runs.

set -eu

if [ -n "${BEND_GATE_NAMES_ONLY:-}" ] || [ -n "${RUST_TOPS_LAWS:-}" ] || [ -n "${RUST_TOPS_ROOT:-}" ]; then
  printf 'bend-proof-ci: BEND_GATE_NAMES_ONLY/RUST_TOPS_LAWS/RUST_TOPS_ROOT must be unset in CI\n' >&2
  exit 1
fi

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

if [ "${1:-}" = "--verdict" ]; then
  if [ -n "${2:-}" ]; then
    printf 'bend-proof-ci: --verdict takes no other argument\n' >&2
    exit 2
  fi
  lock=$root/laws/bend.lock
  if ! command -v bend >/dev/null 2>&1; then
    printf 'bend-gate: bend not on PATH; refusing (H-37 missing_tool_not_green)\n' >&2
    exit 1
  fi
  want=$(awk '
    $0 == "[bend]" { in_s = 1; next }
    /^\[/ { in_s = 0 }
    in_s && $1 == "version" {
      v = $0
      sub(/^[^=]*=[[:space:]]*/, "", v)
      gsub(/"/, "", v)
      print v
      exit
    }
  ' "$lock")
  got_ver=$(bend version | awk 'NR==1 { print $2; exit }')
  if [ -z "$want" ] || [ "$got_ver" != "$want" ]; then
    printf 'bend-proof-ci: bend version %s != lock %s\n' "${got_ver:-missing}" "$want" >&2
    exit 1
  fi
  cd "$root/laws"
  exec bend PROOF.bend --verdict
fi

if [ -n "${1:-}" ]; then
  printf 'bend-proof-ci: unknown argument %s\n' "$1" >&2
  exit 2
fi

exec "$root/scripts/bend-gate.sh"
