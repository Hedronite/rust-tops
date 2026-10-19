#!/bin/bash
# bend-verdict workflow step. pipefail is load-bearing: `time | tee` would
# otherwise report tee's status when the proof exits non-zero.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
out=${VERDICT_OUT:-${RUNNER_TEMP:-/tmp}/verdict.out}
timefile=${VERDICT_TIME:-${RUNNER_TEMP:-/tmp}/verdict-time.txt}
timing=${VERDICT_TIMING_MD:-$root/VERDICT-TIMING.md}

mkdir -p "$(dirname "$out")" "$(dirname "$timefile")" "$(dirname "$timing")"

/usr/bin/time -v -o "$timefile" "$root/scripts/bend-proof-ci.sh" --verdict | tee "$out"
grep -qxF 'ALL PROOFS CHECK' "$out"

elapsed=$(awk -F': ' '/Elapsed \(wall clock\)/ { print $2 }' "$timefile")
rss=$(awk -F': ' '/Maximum resident set size/ { print $2 }' "$timefile")
{
  echo "# Bend verdict timing"
  echo
  echo "- wall_time: ${elapsed}"
  echo "- peak_rss_kbytes: ${rss}"
  echo "- bend: $(bend version | awk 'NR==1 { print $2 }')"
  echo "- bend_sha256_linux_x64: 02089dc0eed0fd5fd6d73c74cc9cffcb2c6638dd3b02ae83f7b8cc57fbe381ba"
  echo "- lean: $(lean --version)"
  echo "- lean_sha256: caaa98356098c85dc0fcbbd28e1ec66f39eb6551829972b752ff20e1286b646b"
  echo "- result: $(cat "$out")"
  echo "- runner: ubuntu-24.04"
} > "$timing"
test -s "$timing"
