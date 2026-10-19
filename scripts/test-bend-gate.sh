#!/bin/sh
# Mutation matrix for the Bend gate and law-diff. Each case runs in a temp copy.
# Every FAIL case asserts its exact message, not only a non-zero status.
# law-diff is the std-only Rust bin, so this matrix does not need a BSD userland.

set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pass=0
fail=0

note() {
  printf '%s\n' "$*"
}

ok() {
  pass=$((pass + 1))
  note "PASS $1"
}

bad() {
  fail=$((fail + 1))
  note "FAIL $1"
  if [ -n "${2:-}" ]; then
    printf '%s\n' "$2"
  fi
}

fresh() {
  dest=$1
  rm -rf "$dest"
  mkdir -p "$dest/crates/cargo-tops/templates"
  cp -a "$root/laws/." "$dest/laws/"
  cp -a "$root/scripts/." "$dest/scripts/"
  cp -a "$root/crates/cargo-tops/templates/LAWS.bend" "$dest/crates/cargo-tops/templates/LAWS.bend"
  chmod +x "$dest/scripts/"*.sh
}

# dash applies set -e to a failing function even after set +e in the caller
# if the callee turns set -e back on. Capture status without that toggle.
run_capture() {
  cap=$1
  shift
  set +e
  "$@" >"$cap.out" 2>"$cap.err"
  rc=$?
  cat "$cap.out" "$cap.err" > "$cap.all"
  return "$rc"
}

expect_ok() {
  name=$1
  shift
  run_capture "$work/cap" "$@"
  rc=$?
  set -e
  if [ "$rc" -eq 0 ]; then
    ok "$name"
  else
    bad "$name" "$(cat "$work/cap.all")"
  fi
}

expect_msg() {
  name=$1
  needle=$2
  shift 2
  run_capture "$work/cap" "$@"
  rc=$?
  set -e
  if [ "$rc" -eq 0 ]; then
    bad "$name expected failure" "$(cat "$work/cap.all")"
    return
  fi
  if grep -F -q -- "$needle" "$work/cap.all"; then
    ok "$name: $needle"
  else
    bad "$name missing [$needle]" "$(cat "$work/cap.all")"
  fi
}

py() {
  python3 - "$@"
}

# a. clean tree
fresh "$work/a"
expect_ok "a clean tree" "$work/a/scripts/bend-gate.sh"

# b. bend removed from PATH
fresh "$work/b"
expect_msg "b" "bend-gate: bend not on PATH; refusing (H-37 missing_tool_not_green)" \
  env PATH=/usr/bin:/bin HOME="$work/home-b" "$work/b/scripts/bend-gate.sh"

# c. linux-x64 sha256 altered by one hex digit
fresh "$work/c"
py << PY
from pathlib import Path
p = Path("$work/c/laws/bend.lock")
text = p.read_text()
old = "02089dc0eed0fd5fd6d73c74cc9cffcb2c6638dd3b02ae83f7b8cc57fbe381ba"
new = "02089dc0eed0fd5fd6d73c74cc9cffcb2c6638dd3b02ae83f7b8cc57fbe381bb"
assert old in text
p.write_text(text.replace(old, new, 1))
PY
expect_msg "c" "fetch-bend: sha256 mismatch for linux-x64" \
  env HOME="$work/home-c" "$work/c/scripts/fetch-bend.sh"

# c'. running platform marked unverified
fresh "$work/c2"
py << PY
from pathlib import Path
p = Path("$work/c2/laws/bend.lock")
text = p.read_text()
needle = "[linux-x64]\n"
assert needle in text
# Flip only the linux-x64 unverified flag, the first one after that header.
head, tail = text.split(needle, 1)
rest, after = tail.split("\n[", 1)
old = "unverified = false"
assert old in rest
rest = rest.replace(old, "unverified = true", 1)
p.write_text(head + needle + rest + "\n[" + after)
PY
expect_msg "c'" "fetch-bend: linux-x64 is unverified; refusing" \
  env HOME="$work/home-c2" "$work/c2/scripts/fetch-bend.sh"

# d. evidence_verdict FromImpl -> Allow. Bend must name the law.
fresh "$work/d"
sync_template() {
  cp -a "$1/laws/LAWS.bend" "$1/crates/cargo-tops/templates/LAWS.bend"
}

CASE=$work/d py << 'PY'
from pathlib import Path
import os
p = Path(os.environ["CASE"]) / "laws/LAWS.bend"
text = p.read_text()
old = "    case FromImpl{}:\n      Refuse{}"
new = "    case FromImpl{}:\n      Allow{}"
assert text.count(old) >= 1
p.write_text(text.replace(old, new, 1))
PY
sync_template "$work/d"
expect_msg "d" "evidence_not_self_served" env HOME="$work/home-d" "$work/d/scripts/bend-gate.sh"

# e. law text restated. Bend type mismatch and law-diff changed-law.
fresh "$work/e-base"
fresh "$work/e"
py << PY
from pathlib import Path
p = Path("$work/e/laws/LAWS.bend")
text = p.read_text()
old = "{evidence_verdict(FromImpl{}) == Refuse{} : Verdict}"
new = "{evidence_verdict(FromContract{}) == Allow{} : Verdict}"
assert old in text
p.write_text(text.replace(old, new, 1))
PY
sync_template "$work/e"
expect_msg "e bend" "Location: main" env HOME="$work/home-e" "$work/e/scripts/bend-gate.sh"
expect_msg "e law-diff" "law-diff: changed law evidence_not_self_served (needs laws-change-approved)" \
  "$root/scripts/law-diff.sh" --base-dir "$work/e-base" --head-dir "$work/e"

# f. law renamed. Gate misses the NAMES row; law-diff reports removed and added.
fresh "$work/f"
py << PY
from pathlib import Path
p = Path("$work/f/laws/LAWS.bend")
text = p.read_text()
old = "law evidence_not_self_served:"
new = "law evidence_not_self_served_renamed:"
assert text.count(old) == 1
p.write_text(text.replace(old, new, 1))
PY
fresh "$work/f-base"
expect_msg "f gate" "bend-gate: LAWS.bend does not state evidence_not_self_served" \
  env HOME="$work/home-f" "$work/f/scripts/bend-gate.sh"
expect_msg "f removed" "law-diff: removed law evidence_not_self_served (needs laws-change-approved)" \
  "$root/scripts/law-diff.sh" --base-dir "$work/f-base" --head-dir "$work/f"
expect_msg "f added" "law-diff: added law evidence_not_self_served_renamed (needs laws-change-approved)" \
  "$root/scripts/law-diff.sh" --base-dir "$work/f-base" --head-dir "$work/f"

# g. delete one law, its NAMES row, and its proof conjunct.
fresh "$work/g-base"
fresh "$work/g"
py << PY
from pathlib import Path
root = Path("$work/g")
laws = (root / "laws/LAWS.bend").read_text()
start = laws.index("type DenySite is Data:")
laws = laws[:start].rstrip() + "\n"
(root / "laws/LAWS.bend").write_text(laws)
names = (root / "laws/NAMES").read_text().splitlines()
names = [n for n in names if n != "single_deny_site"]
(root / "laws/NAMES").write_text("\n".join(names) + "\n")
proof = (root / "laws/PROOF.bend").read_text()
claim = " & {Laws.deny_verdict(Laws.DenyDouble{}) == Laws.Refuse{} : Laws.Verdict}"
assert claim in proof
proof = proof.replace(claim, "", 1)
proof = proof.replace("Laws.single_deny_site()", "Laws.drop_in_is_a_floor_not_a_ceiling()", 1)
(root / "laws/PROOF.bend").write_text(proof)
PY
expect_msg "g" "law-diff: removed law single_deny_site (needs laws-change-approved)" \
  "$root/scripts/law-diff.sh" --base-dir "$work/g-base" --head-dir "$work/g"

# h. coordinated weaken, no label.
fresh "$work/h-base"
fresh "$work/h"
py << PY
from pathlib import Path
root = Path("$work/h")
laws = (root / "laws/LAWS.bend").read_text()
old = "{evidence_verdict(FromImpl{}) == Refuse{} : Verdict}"
new = "{evidence_verdict(FromContract{}) == Allow{} : Verdict}"
assert old in laws
(root / "laws/LAWS.bend").write_text(laws.replace(old, new, 1))
names = (root / "laws/NAMES").read_text().splitlines()
names[0], names[1] = names[1], names[0]
(root / "laws/NAMES").write_text("\n".join(names) + "\n")
proof = (root / "laws/PROOF.bend").read_text()
old_c = "{Laws.evidence_verdict(Laws.FromImpl{}) == Laws.Refuse{} : Laws.Verdict}"
new_c = "{Laws.evidence_verdict(Laws.FromContract{}) == Laws.Allow{} : Laws.Verdict}"
assert old_c in proof
(root / "laws/PROOF.bend").write_text(proof.replace(old_c, new_c, 1))
PY
expect_msg "h" "law-diff: changed law evidence_not_self_served (needs laws-change-approved)" \
  "$root/scripts/law-diff.sh" --base-dir "$work/h-base" --head-dir "$work/h"

# h'. same edit with the label.
expect_ok "h' labeled" env LABELS='["laws-change-approved"]' \
  "$root/scripts/law-diff.sh" --base-dir "$work/h-base" --head-dir "$work/h"

# i. pure append of a new law, NAMES row, and proof conjunct.
fresh "$work/i-base"
fresh "$work/i"
CASE=$work/i py << 'PY'
from pathlib import Path
import os
root = Path(os.environ["CASE"])
laws = (root / "laws/LAWS.bend").read_text()
if not laws.endswith("\n"):
    laws += "\n"
laws += """
type HaloWitness is Data:
  HaloOk{}

def halo_witness_verdict(x: HaloWitness) -> Verdict:
  match x:
    case HaloOk{}:
      Allow{}

law halo_witness_appended:
  {halo_witness_verdict(HaloOk{}) == Allow{} : Verdict}

def halo_witness_appended():
  {==}
"""
(root / "laws/LAWS.bend").write_text(laws)
names = (root / "laws/NAMES").read_text()
if not names.endswith("\n"):
    names += "\n"
(root / "laws/NAMES").write_text(names + "halo_witness_appended\n")
proof = (root / "laws/PROOF.bend").read_text()
lines = proof.splitlines(keepends=True)
found = False
for i, line in enumerate(lines):
    if line.startswith("def main("):
        nl = "\n" if line.endswith("\n") else ""
        body = line.rstrip("\n")
        suffix = ": Laws.Verdict}:"
        if not body.endswith(suffix):
            raise SystemExit("main signature suffix drifted: " + body[-40:])
        body = body[: -len(suffix)] + ": Laws.Verdict} & {Laws.halo_witness_verdict(Laws.HaloOk{}) == Laws.Allow{} : Laws.Verdict}:"
        lines[i] = body + nl
        found = True
        break
if not found:
    raise SystemExit("no main")
text = "".join(lines)
old_call = "Laws.single_deny_site()"
new_call = "(Laws.single_deny_site(), Laws.halo_witness_appended())"
assert text.count(old_call) == 1
(root / "laws/PROOF.bend").write_text(text.replace(old_call, new_call, 1))
PY
sync_template "$work/i"
expect_ok "i law-diff" "$root/scripts/law-diff.sh" --base-dir "$work/i-base" --head-dir "$work/i"
expect_ok "i bend" env HOME="$work/home-i" "$work/i/scripts/bend-gate.sh"

# i'. block inserted mid-file
fresh "$work/ip-base"
fresh "$work/ip"
py << PY
from pathlib import Path
p = Path("$work/ip/laws/LAWS.bend")
text = p.read_text()
needle = "law evidence_not_self_served:\n"
insert = needle + "  {evidence_verdict(FromImpl{}) == Refuse{} : Verdict}\n\nlaw halo_mid_insert:\n  {evidence_verdict(FromImpl{}) == Refuse{} : Verdict}\n\n"
assert needle in text
p.write_text(text.replace(needle, insert, 1))
PY
expect_msg "i'" "law-diff: non-append change in laws/LAWS.bend" \
  "$root/scripts/law-diff.sh" --base-dir "$work/ip-base" --head-dir "$work/ip"

# j. template differs by one byte. Gate message, and the byte equality the cargo test asserts.
fresh "$work/j"
py << PY
from pathlib import Path
p = Path("$work/j/crates/cargo-tops/templates/LAWS.bend")
data = bytearray(p.read_bytes())
data[-1] = (data[-1] + 1) % 256
p.write_bytes(data)
PY
expect_msg "j" "bend-gate: template drift (laws/LAWS.bend != crates/cargo-tops/templates/LAWS.bend)" \
  env HOME="$work/home-j" "$work/j/scripts/bend-gate.sh"
if cmp -s "$work/j/laws/LAWS.bend" "$work/j/crates/cargo-tops/templates/LAWS.bend"; then
  bad "j cargo byte equality still holds"
else
  ok "j cargo laws_bend_matches_pack would fail (profiles::LAWS_BEND != laws/LAWS.bend)"
fi

# k. verdict on all 53 laws
fresh "$work/k"
if run_capture "$work/cap" env HOME="${HOME}" "$work/k/scripts/bend-proof-ci.sh" --verdict; then
  if grep -F -q "ALL PROOFS CHECK" "$work/cap.out"; then
    ok "k ALL PROOFS CHECK"
  else
    bad "k missing ALL PROOFS CHECK" "$(cat "$work/cap.all")"
  fi
else
  bad "k verdict failed" "$(cat "$work/cap.all")"
fi

# l. verdict-function body changed, law text untouched
fresh "$work/l-base"
fresh "$work/l"
py << PY
from pathlib import Path
p = Path("$work/l/laws/LAWS.bend")
text = p.read_text()
old = "    case FromImpl{}:\n      Refuse{}"
new = "    case FromImpl{}:\n      Allow{}"
assert old in text
p.write_text(text.replace(old, new, 1))
PY
expect_msg "l" "law-diff: non-append change in laws/LAWS.bend" \
  "$root/scripts/law-diff.sh" --base-dir "$work/l-base" --head-dir "$work/l"

# l'. a constructor added to Verdict
fresh "$work/lp-base"
fresh "$work/lp"
py << PY
from pathlib import Path
p = Path("$work/lp/laws/LAWS.bend")
text = p.read_text()
old = "type Verdict is Data:\n  Refuse{}\n  Ack{}\n  Allow{}"
new = "type Verdict is Data:\n  Refuse{}\n  Ack{}\n  Allow{}\n  Extra{}"
assert old in text
p.write_text(text.replace(old, new, 1))
PY
expect_msg "l'" "law-diff: non-append change in laws/LAWS.bend" \
  "$root/scripts/law-diff.sh" --base-dir "$work/lp-base" --head-dir "$work/lp"

# l''. append that re-declares an existing def
fresh "$work/lpp-base"
fresh "$work/lpp"
py << PY
from pathlib import Path
p = Path("$work/lpp/laws/LAWS.bend")
text = p.read_text()
if not text.endswith("\n"):
    text += "\n"
text += "\ndef evidence_verdict(x: Evidence) -> Verdict:\n  Allow{}\n"
p.write_text(text)
PY
expect_msg "l''" "law-diff: append re-declares existing name evidence_verdict" \
  "$root/scripts/law-diff.sh" --base-dir "$work/lpp-base" --head-dir "$work/lpp"

# m. each downgrade env var, under bend-proof-ci.sh
fresh "$work/m"
expect_msg "m NAMES_ONLY" "bend-proof-ci: BEND_GATE_NAMES_ONLY/RUST_TOPS_LAWS must be unset in CI" \
  env BEND_GATE_NAMES_ONLY=1 "$work/m/scripts/bend-proof-ci.sh"
expect_msg "m RUST_TOPS_LAWS" "bend-proof-ci: BEND_GATE_NAMES_ONLY/RUST_TOPS_LAWS must be unset in CI" \
  env RUST_TOPS_LAWS="$work/other-laws" "$work/m/scripts/bend-proof-ci.sh"

# n. law X is a strict prefix of an existing name, present in LAWS and NAMES, absent from PROOF
fresh "$work/n"
py << PY
from pathlib import Path
root = Path("$work/n")
laws = (root / "laws/LAWS.bend").read_text()
if not laws.endswith("\n"):
    laws += "\n"
laws += """
law evidence_not:
  {evidence_verdict(FromImpl{}) == Refuse{} : Verdict}

def evidence_not():
  {==}
"""
(root / "laws/LAWS.bend").write_text(laws)
names = (root / "laws/NAMES").read_text()
if not names.endswith("\n"):
    names += "\n"
(root / "laws/NAMES").write_text(names + "evidence_not\n")
PY
expect_msg "n" "bend-gate: proof does not name evidence_not" \
  env HOME="$work/home-n" "$work/n/scripts/bend-gate.sh"

note "----"
note "passed=$pass failed=$fail"
if [ "$fail" -ne 0 ]; then
  exit 1
fi
