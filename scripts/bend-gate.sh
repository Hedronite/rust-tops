#!/bin/sh
# Gate the provisional rust-tops Bend pack. Does not amend protocol 1.0.
# Always asserts every harvest law is named in LAWS.bend and PROOF.bend.
# If bend is on PATH, runs the shipped proof (a copy missing one law fails).

set -eu
export BEND_NO_TELEMETRY="${BEND_NO_TELEMETRY:-1}"

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
laws_dir=${RUST_TOPS_LAWS:-"$root/laws"}
laws="$laws_dir/LAWS.bend"
proof=${1:-"$laws_dir/PROOF.bend"}
names="$laws_dir/NAMES"

if [ ! -f "$laws" ] || [ ! -f "$proof" ] || [ ! -f "$names" ]; then
  printf 'bend-gate: missing laws pack under %s\n' "$laws_dir" >&2
  exit 2
fi

expected=$(grep -c . "$names")
got=$(grep -c '^law ' "$laws" || true)
if [ "$got" -ne "$expected" ]; then
  printf 'bend-gate: LAWS.bend has %s law statements, expected %s\n' "$got" "$expected" >&2
  exit 1
fi

while IFS= read -r law; do
  [ -n "$law" ] || continue
  if ! grep -q "^law ${law}:" "$laws"; then
    printf 'bend-gate: LAWS.bend does not state %s\n' "$law" >&2
    exit 1
  fi
  if ! grep -q "$law" "$proof"; then
    printf 'bend-gate: proof does not name %s\n' "$law" >&2
    exit 1
  fi
done < "$names"

if [ "${BEND_GATE_NAMES_ONLY:-0}" = "1" ]; then
  printf 'bend-gate: %s laws named\n' "$expected"
  exit 0
fi

if ! command -v bend >/dev/null 2>&1; then
  printf 'bend-gate: %s laws named; bend not on PATH (names-only)\n' "$expected"
  exit 0
fi

dir=$(CDPATH= cd -- "$(dirname "$proof")" && pwd)
name=$(basename "$proof")
cd "$dir"
exec bend "$name"
