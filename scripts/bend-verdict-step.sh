#!/bin/bash
# bend-verdict workflow step. pipefail is load-bearing: `time | tee` would
# otherwise report tee's status when the proof exits non-zero.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
out=${VERDICT_OUT:-${RUNNER_TEMP:-/tmp}/verdict.out}
timefile=${VERDICT_TIME:-${RUNNER_TEMP:-/tmp}/verdict-time.txt}
timing=${VERDICT_TIMING_MD:-$root/VERDICT-TIMING.md}

mkdir -p "$(dirname "$out")" "$(dirname "$timefile")" "$(dirname "$timing")"

set +e
/usr/bin/time -v -o "$timefile" "$root/scripts/bend-proof-ci.sh" --verdict | tee "$out"
# Copy PIPESTATUS before any other command: the next assignment resets it.
pipe_rcs=("${PIPESTATUS[@]}")
set -euo pipefail
proof_rc=${pipe_rcs[0]}
tee_rc=${pipe_rcs[1]}
if [ "$proof_rc" -ne 0 ]; then
  printf 'bend-verdict: proof exited %s\n' "$proof_rc" >&2
  exit "$proof_rc"
fi
if [ "$tee_rc" -ne 0 ]; then
  printf 'bend-verdict: tee exited %s\n' "$tee_rc" >&2
  exit "$tee_rc"
fi
if ! grep -qxF 'ALL PROOFS CHECK' "$out"; then
  printf 'bend-verdict: missing ALL PROOFS CHECK\n' >&2
  exit 1
fi

lock=$root/laws/bend.lock
lock_get() {
  section=$1
  key=$2
  awk -v section="$section" -v key="$key" '
    $0 == "[" section "]" { in_s = 1; next }
    /^\[/ { in_s = 0 }
    in_s && $1 == key {
      v = $0
      sub(/^[^=]*=[[:space:]]*/, "", v)
      gsub(/"/, "", v)
      print v
      exit
    }
  ' "$lock"
}
bend_sha=$(lock_get linux-x64 sha256)
lean_sha=$(lock_get lean.linux-x64 sha256)
if [ -z "$bend_sha" ] || [ -z "$lean_sha" ]; then
  printf 'bend-verdict: missing sha256 in %s\n' "$lock" >&2
  exit 1
fi

elapsed=$(awk -F': ' '/Elapsed \(wall clock\)/ { print $2 }' "$timefile")
rss=$(awk -F': ' '/Maximum resident set size/ { print $2 }' "$timefile")
{
  echo "# Bend verdict timing"
  echo
  echo "- wall_time: ${elapsed}"
  echo "- peak_rss_kbytes: ${rss}"
  echo "- bend: $(bend version | awk 'NR==1 { print $2 }')"
  echo "- bend_sha256_linux_x64: ${bend_sha}"
  echo "- lean: $(lean --version)"
  echo "- lean_sha256: ${lean_sha}"
  echo "- result: $(cat "$out")"
  echo "- runner: ubuntu-24.04"
} > "$timing"
test -s "$timing"
