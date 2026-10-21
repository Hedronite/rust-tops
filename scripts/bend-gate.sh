#!/bin/sh
# Gate the provisional rust-tops Bend pack. Does not amend protocol 1.0.
# Always asserts every harvest law is named in LAWS.bend and PROOF.bend.
# A missing bend is a failure (H-37 missing_tool_not_green), not a pass.
# The verdict is the exit code of `bend PROOF.bend` only.

set -eu
export BEND_NO_TELEMETRY="${BEND_NO_TELEMETRY:-1}"

if [ -n "${RUST_TOPS_ROOT:-}" ]; then
  # Nix points the gate at its source tree this way and does not set CI.
  if [ -n "${CI:-}" ]; then
    printf 'bend-gate: RUST_TOPS_ROOT must be unset in CI\n' >&2
    exit 1
  fi
  root=$RUST_TOPS_ROOT
else
  root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
fi
laws_dir=${RUST_TOPS_LAWS:-"$root/laws"}
laws="$laws_dir/LAWS.bend"
proof=${1:-"$laws_dir/PROOF.bend"}
names="$laws_dir/NAMES"
template=$root/crates/cargo-tops/templates/LAWS.bend
lock=$root/laws/bend.lock

if [ ! -f "$laws" ] || [ ! -f "$proof" ] || [ ! -f "$names" ]; then
  printf 'bend-gate: missing laws pack under %s\n' "$laws_dir" >&2
  exit 2
fi

expected=$(awk 'NF { n++ } END { print n+0 }' "$names")
got=$(awk '/^law / { n++ } END { print n+0 }' "$laws")
if [ "$got" -ne "$expected" ]; then
  printf 'bend-gate: LAWS.bend has %s law statements, expected %s\n' "$got" "$expected" >&2
  exit 1
fi

while IFS= read -r law || [ -n "$law" ]; do
  [ -n "$law" ] || continue
  if ! grep -qxF "law ${law}:" "$laws"; then
    printf 'bend-gate: LAWS.bend does not state %s\n' "$law" >&2
    exit 1
  fi
  if ! grep -qwF -- "$law" "$proof"; then
    printf 'bend-gate: proof does not name %s\n' "$law" >&2
    exit 1
  fi
done < "$names"

if [ ! -f "$template" ] || ! cmp -s "$root/laws/LAWS.bend" "$template"; then
  printf 'bend-gate: template drift (laws/LAWS.bend != crates/cargo-tops/templates/LAWS.bend)\n' >&2
  exit 1
fi

if [ "${BEND_GATE_NAMES_ONLY:-0}" = "1" ]; then
  printf 'NAMES-ONLY (not a proof)\n'
  exit 0
fi

if ! command -v bend >/dev/null 2>&1; then
  printf 'bend-gate: bend not on PATH; refusing (H-37 missing_tool_not_green)\n' >&2
  exit 1
fi

if [ ! -f "$lock" ]; then
  printf 'bend-gate: missing %s\n' "$lock" >&2
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
  printf 'bend-gate: bend version %s != lock %s\n' "${got_ver:-missing}" "$want" >&2
  exit 1
fi

dir=$(CDPATH= cd -- "$(dirname "$proof")" && pwd)
name=$(basename "$proof")
cd "$dir"
exec bend "$name"
