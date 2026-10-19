#!/bin/sh
# CI entry for the law ownership diff.
#   law-diff.sh BASE_SHA HEAD_SHA
#   law-diff.sh --base-dir DIR --head-dir DIR
# LABELS is a JSON array (github.event.pull_request.labels.*.name via toJSON).
# The label is parsed with jq. It is never interpolated into the shell.

set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
bin=$tmp/law-diff
rustc --edition 2021 -D warnings -O -o "$bin" "$root/scripts/law-diff.rs"

approved=0
if [ -n "${LABELS:-}" ]; then
  if printf '%s\n' "$LABELS" | jq -e 'index("laws-change-approved") != null' >/dev/null; then
    approved=1
  fi
fi

run_bin() {
  set +e
  "$bin" "$@"
  rc=$?
  set -e
  exit "$rc"
}

if [ "${1:-}" = "--base-dir" ]; then
  if [ "$approved" -eq 1 ]; then
    run_bin --approved "$@"
  fi
  run_bin "$@"
fi

if [ "$#" -ne 2 ]; then
  printf 'law-diff: usage: law-diff.sh BASE_SHA HEAD_SHA\n' >&2
  exit 2
fi

base_sha=$1
head_sha=$2

require_commit() {
  role=$1
  sha=$2
  if ! git -C "$root" rev-parse --verify "${sha}^{commit}" >/dev/null 2>&1; then
    printf 'law-diff: %s SHA does not resolve to a commit: %s\n' "$role" "$sha" >&2
    exit 1
  fi
}

require_commit base "$base_sha"
require_commit head "$head_sha"

extract() {
  sha=$1
  dest=$2
  mkdir -p "$dest/laws"
  for rel in laws/LAWS.bend laws/NAMES laws/PROOF.bend; do
    if git -C "$root" cat-file -e "$sha:$rel" 2>/dev/null; then
      git -C "$root" show "$sha:$rel" > "$dest/$rel"
    else
      : > "$dest/$rel"
    fi
  done
}

extract "$base_sha" "$tmp/base"
extract "$head_sha" "$tmp/head"

if [ "$approved" -eq 1 ]; then
  run_bin --approved --base-dir "$tmp/base" --head-dir "$tmp/head"
fi
run_bin --base-dir "$tmp/base" --head-dir "$tmp/head"
