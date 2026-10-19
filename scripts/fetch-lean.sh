#!/bin/sh
# Install the sha256-pinned Lean toolchain archive. No elan installer, no curl|sh.
# Bend resolves lean/leanc from ~/.elan/toolchains/leanprover--lean4---v<ver>/bin
# before PATH. We fill that directory from the lock, then put it on PATH.
# Every run re-checks the archive sha256, including after a cache restore.

set -eu

if [ -n "${RUST_TOPS_ROOT:-}" ]; then
  root=$RUST_TOPS_ROOT
else
  root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
fi
lock=$root/laws/bend.lock

if [ ! -f "$lock" ]; then
  printf 'fetch-lean: missing %s\n' "$lock" >&2
  exit 1
fi

lock_get() {
  section=$1
  key=$2
  awk -v section="$section" -v key="$key" '
    $0 == "[" section "]" { in_s = 1; next }
    /^\[/ { in_s = 0 }
    in_s {
      line = $0
      sub(/^[[:space:]]+/, "", line)
      eq = index(line, "=")
      if (eq == 0) next
      k = substr(line, 1, eq - 1)
      gsub(/[[:space:]]/, "", k)
      if (k != key) next
      v = substr(line, eq + 1)
      sub(/^[[:space:]]+/, "", v)
      sub(/[[:space:]]+$/, "", v)
      if (substr(v, 1, 1) == "\"" && substr(v, length(v), 1) == "\"") {
        v = substr(v, 2, length(v) - 2)
      }
      print v
      exit
    }
  ' "$lock"
}

os=$(uname -s)
mach=$(uname -m)
case "$os:$mach" in
  Linux:x86_64) plat=linux-x64 ;;
  *)
    printf 'fetch-lean: unlisted platform %s:%s\n' "$os" "$mach" >&2
    exit 1
    ;;
esac

section="lean.$plat"
ver=$(lock_get "$section" version)
sha=$(lock_get "$section" sha256)
url=$(lock_get "$section" source_url)
archive_name=$(lock_get "$section" archive)
flag=$(lock_get "$section" unverified)
if [ -z "$ver" ] || [ -z "$sha" ] || [ -z "$url" ] || [ -z "$archive_name" ]; then
  printf 'fetch-lean: unlisted platform %s\n' "$plat" >&2
  exit 1
fi
if [ "$flag" = "true" ]; then
  printf 'fetch-lean: %s is unverified; refusing\n' "$plat" >&2
  exit 1
fi
case "$url" in
  https://*) ;;
  *)
    printf 'fetch-lean: refusing non-https source for %s\n' "$plat" >&2
    exit 1
    ;;
esac

cache_dir=${LEAN_ARCHIVE_DIR:-"$HOME/.cache/rust-tops/lean"}
mkdir -p "$cache_dir"
archive=$cache_dir/$archive_name

sha_ok() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s  %s\n' "$sha" "$archive" | sha256sum -c - >/dev/null
  else
    printf '%s  %s\n' "$sha" "$archive" | shasum -a 256 -c - >/dev/null
  fi
}

if [ -f "$archive" ]; then
  if ! sha_ok; then
    printf 'fetch-lean: cached archive failed sha256; re-downloading\n' >&2
    rm -f "$archive"
  fi
fi
if [ ! -f "$archive" ]; then
  curl --proto '=https' --tlsv1.2 -fsSL -o "$archive.partial" "$url"
  mv "$archive.partial" "$archive"
fi
if ! sha_ok; then
  printf 'fetch-lean: sha256 mismatch for %s\n' "$plat" >&2
  exit 1
fi

tool_dir=$HOME/.elan/toolchains/leanprover--lean4---v$ver
rm -rf "$tool_dir"
mkdir -p "$HOME/.elan/toolchains"
stage=$(mktemp -d)
tar -I zstd -xf "$archive" -C "$stage"
src=$stage/lean-$ver-linux
if [ ! -x "$src/bin/lean" ] || [ ! -x "$src/bin/leanc" ]; then
  printf 'fetch-lean: archive layout missing bin/lean or bin/leanc\n' >&2
  rm -rf "$stage"
  exit 1
fi
mv "$src" "$tool_dir"
rm -rf "$stage"

if [ -n "${GITHUB_PATH:-}" ]; then
  printf '%s\n' "$tool_dir/bin" >> "$GITHUB_PATH"
fi
export PATH="$tool_dir/bin:$PATH"
got=$("$tool_dir/bin/lean" --version)
case "$got" in
  *"version $ver"*) ;;
  *)
    printf 'fetch-lean: lean --version %s != lock %s\n' "$got" "$ver" >&2
    exit 1
    ;;
esac
printf 'fetch-lean: installed lean %s to %s\n' "$ver" "$tool_dir"
