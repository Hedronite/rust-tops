#!/bin/sh
# CI entry for the law ownership diff.
#   law-diff.sh BASE_SHA HEAD_SHA
#   law-diff.sh --base-dir DIR --head-dir DIR
# LABELS is a JSON array (github.event.pull_request.labels.*.name via toJSON).
# The label is parsed with jq. It is never interpolated into the shell.

set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
bin=$root/target/law-diff
mkdir -p "$root/target"
if [ ! -x "$bin" ] || [ "$root/scripts/law-diff.rs" -nt "$bin" ]; then
  rustc --edition 2021 -D warnings -O -o "$bin" "$root/scripts/law-diff.rs"
fi

approved=0
if [ -n "${LABELS:-}" ]; then
  if printf '%s\n' "$LABELS" | jq -e 'index("laws-change-approved") != null' >/dev/null; then
    approved=1
  fi
fi

if [ "${1:-}" = "--base-dir" ]; then
  if [ "$approved" -eq 1 ]; then
    exec "$bin" --approved "$@"
  fi
  exec "$bin" "$@"
fi

if [ "$#" -ne 2 ]; then
  printf 'law-diff: usage: law-diff.sh BASE_SHA HEAD_SHA\n' >&2
  exit 2
fi

base_sha=$1
head_sha=$2
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

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
  set +e
  "$bin" --approved --base-dir "$tmp/base" --head-dir "$tmp/head"
  rc=$?
  set -e
  exit "$rc"
fi
set +e
"$bin" --base-dir "$tmp/base" --head-dir "$tmp/head"
rc=$?
set -e
exit "$rc"
