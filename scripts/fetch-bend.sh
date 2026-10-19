#!/bin/sh
# Download the TOFU-pinned Bend release, verify sha256, install to $HOME/.bend.
# All fetch / verify / refuse logic lives here. The setup-bend action only calls it.
# Never pipes the download into a shell. Never calls `bend update`.

set -eu
export BEND_NO_TELEMETRY=1

if [ -n "${RUST_TOPS_ROOT:-}" ]; then
  root=$RUST_TOPS_ROOT
else
  root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
fi
lock=$root/laws/bend.lock

if [ ! -f "$lock" ]; then
  printf 'fetch-bend: missing %s\n' "$lock" >&2
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
  Linux:aarch64 | Linux:arm64) plat=linux-arm64 ;;
  Darwin:arm64 | Darwin:aarch64) plat=darwin-arm64 ;;
  Darwin:x86_64) plat=darwin-x64 ;;
  *)
    printf 'fetch-bend: unlisted platform %s:%s\n' "$os" "$mach" >&2
    exit 1
    ;;
esac

lock_ver=$(lock_get bend version)
if [ -z "$lock_ver" ]; then
  printf 'fetch-bend: unlisted version\n' >&2
  exit 1
fi
if [ -n "${1:-}" ] && [ "$1" != "$lock_ver" ]; then
  printf 'fetch-bend: unlisted version %s\n' "$1" >&2
  exit 1
fi

sha=$(lock_get "$plat" sha256)
url=$(lock_get "$plat" source_url)
flag=$(lock_get "$plat" unverified)
if [ -z "$sha" ] || [ -z "$url" ]; then
  printf 'fetch-bend: unlisted platform %s\n' "$plat" >&2
  exit 1
fi
if [ "$flag" = "true" ]; then
  printf 'fetch-bend: %s is unverified; refusing\n' "$plat" >&2
  exit 1
fi
case "$url" in
  https://*) ;;
  *)
    printf 'fetch-bend: refusing non-https source for %s\n' "$plat" >&2
    exit 1
    ;;
esac

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
# Download to a file. Do not pipe into a shell.
curl --proto '=https' --tlsv1.2 -fsSL -o "$tmp" "$url"

if command -v sha256sum >/dev/null 2>&1; then
  if ! printf '%s  %s\n' "$sha" "$tmp" | sha256sum -c - >/dev/null; then
    printf 'fetch-bend: sha256 mismatch for %s\n' "$plat" >&2
    exit 1
  fi
else
  if ! printf '%s  %s\n' "$sha" "$tmp" | shasum -a 256 -c - >/dev/null; then
    printf 'fetch-bend: sha256 mismatch for %s\n' "$plat" >&2
    exit 1
  fi
fi

dest=$HOME/.bend
rm -rf "$dest"
mkdir -p "$dest"
tar -xzf "$tmp" -C "$dest" --strip-components=1
if [ ! -x "$dest/bin/bend" ] || [ ! -d "$dest/bend2" ] || [ ! -d "$dest/guide" ]; then
  printf 'fetch-bend: archive layout missing bin/bend, bend2/, or guide/\n' >&2
  exit 1
fi

if [ -n "${GITHUB_PATH:-}" ]; then
  printf '%s\n' "$dest/bin" >> "$GITHUB_PATH"
fi
if [ -n "${GITHUB_ENV:-}" ]; then
  printf 'BEND_NO_TELEMETRY=1\n' >> "$GITHUB_ENV"
fi
export PATH="$dest/bin:$PATH"
printf 'fetch-bend: installed %s to %s\n' "$lock_ver" "$dest"
