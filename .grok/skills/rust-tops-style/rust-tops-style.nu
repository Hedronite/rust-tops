#!/usr/bin/env nu
# MIT. Rules distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. Copyright (c) 2026 Morgan Linton. See LICENSES/BeautifulRust-MIT.txt.
# Style check for a Rust tree. Clippy decides the lint rules and the probes below
# decide the rest. The catalog is rules.toml beside this script; this script never
# edits it. The output is one JSON object, with no time, host or absolute path in
# it, so two runs on one tree give the same bytes. It does not emit a verdict.

const HERE = (path self | path dirname)
const DEFAULT_NU = "0.108.0"
const SELF = "nu rust-tops-style.nu"

def fail [command: string, error: string] {
  {ok: false, command: $command, error: ($error | str trim | str substring 0..<400)} | to json
}

def quote-arg [a: string] {
  if ($a | str contains " ") { $"\"($a)\"" } else { $a }
}

def argv-line [args: list<string>] {
  $args | each {|a| quote-arg $a } | str join " "
}

def have [bin: string] {
  not ((which $bin | where type == external) | is-empty)
}

# [major minor patch] of a version string, or null.
def semver [v: string] {
  let m = ($v | parse --regex '^(?<a>\d+)\.(?<b>\d+)(?:\.(?<c>\d+))?')
  if ($m | is-empty) { return null }
  let p = ($m | first)
  [($p.a | into int) ($p.b | into int) (if ($p.c | is-empty) { 0 } else { $p.c | into int })]
}

# The same major, and not older than the lock asks.
def nu-ok [required: string, found: string] {
  let r = (semver $required)
  let f = (semver $found)
  if $r == null or $f == null { return false }
  if $f.0 != $r.0 { return false }
  $f.1 > $r.1 or ($f.1 == $r.1 and $f.2 >= $r.2)
}

# Nearest file called `name` at or above `start`, or null.
def find-up [start: string, name: string] {
  mut dir = ($start | path expand)
  loop {
    let cand = ($dir | path join $name)
    if ($cand | path exists) { return $cand }
    let parent = ($dir | path dirname)
    if $parent == $dir { return null }
    $dir = $parent
  }
}

def required-nu [start: string] {
  let lock = (find-up $start "rust-tops.lock")
  if $lock == null { return $DEFAULT_NU }
  let doc = (try { open --raw $lock | from toml } catch { {} })
  $doc | get -o tools.nushell | default $DEFAULT_NU
}

# --- probes -----------------------------------------------------------------
# A probe takes one file as rows {file, n, text, prev} (n is the 1-based line, prev
# the line above) and returns {line, col, message} rows. Builtins only.

const FN_PUB = '\bpub (?:(?:const|async|unsafe) )*fn\b'

# 1-based column, counted in Unicode scalars as clippy counts it.
def col-of [text: string, needle: string] {
  let b = ($text | str index-of --utf-8-bytes $needle)
  1 + ($text | encode utf8 | bytes at 0..<$b | decode utf-8 | str length --chars)
}

def is-comment [text: string] {
  $text | str trim --left | str starts-with "//"
}

def is-doc [text: string] {
  let t = ($text | str trim --left)
  ($t | str starts-with "///") or ($t | str starts-with "//!")
}

def is-bin [file: string] {
  ($file | path basename) == "main.rs" or ($file | str starts-with "src/bin/")
}

def hit [r: record, needle: string, message: string] {
  {line: $r.n, col: (col-of $r.text $needle), message: $message}
}

# Rows that are code (not a `//` line) and match `re`.
def code-matching [rows: list, re: string] {
  $rows | where {|r| (not (is-comment $r.text)) and ($r.text =~ $re) }
}

def probe-box-dyn-error-api [rows: list] {
  $rows
  | where {|r| (not (is-bin $r.file)) and (not (is-comment $r.text)) and ($r.text =~ $FN_PUB) and ($r.text =~ 'Box<dyn (?:std::|core::)?(?:error::)?Error') }
  | each {|r| hit $r "Box<dyn" "public fn exposes a boxed dynamic error" }
}

def probe-anyhow-in-api [rows: list] {
  $rows
  | where {|r| (not (is-bin $r.file)) and (not (is-comment $r.text)) and ($r.text =~ $FN_PUB) and ($r.text =~ 'anyhow::(?:Result|Error)') }
  | each {|r| hit $r "anyhow::" "public fn exposes an anyhow type" }
}

def probe-catch-unwind [rows: list] {
  (code-matching $rows '\bcatch_unwind\(')
  | where {|r| not (is-comment $r.prev) }
  | each {|r| hit $r "catch_unwind" "catch_unwind with no comment naming the boundary" }
}

def probe-pub-bool-param [rows: list] {
  (code-matching $rows ($FN_PUB + '[^)]*\(.*\w\s*:\s*bool\b'))
  | each {|r| hit $r ": bool" "public fn takes a bool parameter" }
}

def probe-get-prefix-getter [rows: list] {
  (code-matching $rows '\bfn get_\w+\(&self\)')
  | each {|r| hit $r "fn get_" "getter has a get_ prefix" }
}

def probe-vague-type-name [rows: list] {
  $rows
  | where {|r| not (is-comment $r.text) }
  | each {|r|
      let m = ($r.text | parse --regex '\b(?:struct|enum|trait|type|union)\s+(?<name>\w*(?:Manager|Util|Utils|Helper))\b')
      if ($m | is-empty) { null } else { hit $r ($m | first | get name) "type name says nothing about what it models" }
    }
  | compact
}

def probe-transmute-use [rows: list] {
  (code-matching $rows '\btransmute(?:::<.*>)?\(')
  | each {|r| hit $r "transmute" "transmute call" }
}

def probe-unsafe-impl-send-sync [rows: list] {
  (code-matching $rows '\bunsafe impl\b[^{]*\b(?:Send|Sync) for\b')
  | where {|r| not ($r.prev | str trim --left | str starts-with "// SAFETY") }
  | each {|r| hit $r "unsafe impl" "unsafe impl of Send or Sync with no SAFETY comment" }
}

def probe-doc-example-unwrap [rows: list] {
  $rows
  | where {|r| (is-doc $r.text) and ($r.text | str contains ".unwrap()") }
  | each {|r| hit $r ".unwrap()" "doc example calls unwrap" }
}

def probe-deny-warnings-attr [rows: list] {
  (code-matching $rows '^\s*#!?\[(?:deny|forbid)\([^)]*\bwarnings\b')
  | each {|r| hit $r "deny(" "attribute denies the warnings group" }
}

def probe-crate-lint-attr [rows: list] {
  (code-matching $rows '^\s*#!\[(?:allow|warn|deny|forbid|expect)\(')
  | each {|r| hit $r "#![" "crate-level lint attribute" }
}

def probe-poison-into-inner [rows: list] {
  (code-matching $rows '\.into_inner\(\)')
  | where {|r| ($r.text | str contains "unwrap_or_else") or ($r.text | str contains "oison") }
  | each {|r| hit $r ".into_inner()" "into_inner recovers a poisoned lock" }
}

const PROBES = [
  "anyhow-in-api" "box-dyn-error-api" "catch-unwind" "crate-lint-attr" "deny-warnings-attr"
  "doc-example-unwrap" "get-prefix-getter" "poison-into-inner" "pub-bool-param"
  "transmute-use" "unsafe-impl-send-sync" "vague-type-name"
]

def run-probe [name: string, rows: list] {
  match $name {
    "anyhow-in-api" => (probe-anyhow-in-api $rows)
    "box-dyn-error-api" => (probe-box-dyn-error-api $rows)
    "catch-unwind" => (probe-catch-unwind $rows)
    "crate-lint-attr" => (probe-crate-lint-attr $rows)
    "deny-warnings-attr" => (probe-deny-warnings-attr $rows)
    "doc-example-unwrap" => (probe-doc-example-unwrap $rows)
    "get-prefix-getter" => (probe-get-prefix-getter $rows)
    "poison-into-inner" => (probe-poison-into-inner $rows)
    "pub-bool-param" => (probe-pub-bool-param $rows)
    "transmute-use" => (probe-transmute-use $rows)
    "unsafe-impl-send-sync" => (probe-unsafe-impl-send-sync $rows)
    "vague-type-name" => (probe-vague-type-name $rows)
    _ => []
  }
}

def rust-files [root: string] {
  glob ($root | path join "**/*.rs")
  | each {|f| $f | path relative-to $root }
  | where {|rel| not ($rel | path split | any {|c| $c in ["target" ".git" "node_modules" "vendor"] }) }
  | sort
}

# --- clippy ------------------------------------------------------------------

# `file_name` is relative to the workspace root (or absolute). Returns the path
# relative to `root`, or null when it lies outside `root`.
def under-root [file_name: string, ws: string, root: string] {
  let abs = if ($file_name | str starts-with "/") { $file_name } else { $ws | path join $file_name }
  let abs = if ($abs | str contains "..") { $abs | path expand } else { $abs }
  try { $abs | path relative-to $root } catch { null }
}

def lint-findings [msgs: list, lints: list, ws: string, root: string] {
  $msgs
  | where {|m| ($m.message | get -o code.code | default "") in ($lints | get check) }
  | each {|m|
      let rule = ($lints | where check == ($m.message | get code.code) | first)
      let spans = ($m.message.spans | where is_primary == true)
      if ($spans | is-empty) { null } else {
        let s = ($spans | first)
        let rel = (under-root $s.file_name $ws $root)
        if $rel == null { null } else {
          {
            rule: $rule.id
            kind: "lint"
            file: $rel
            line: $s.line_start
            end: $s.line_end
            col: $s.column_start
            message: $m.message.message
            fix: $rule.fix
          }
        }
      }
    }
  | compact
}

# --- --base ------------------------------------------------------------------

# Rows {file, start, end} for the lines `base...HEAD` adds or changes.
def changed-ranges [diff: string] {
  mut cur = ""
  mut rows = []
  for l in ($diff | lines) {
    if ($l | str starts-with "+++ ") {
      let p = ($l | str substring 4.. | str trim --right --char "\t")
      $cur = if $p == "/dev/null" { "" } else if ($p | str starts-with "b/") { $p | str substring 2.. } else { $p }
    } else if $cur != "" and ($l | str starts-with "@@ ") {
      let m = ($l | parse --regex '^@@ -\d+(?:,\d+)? \+(?<start>\d+)(?:,(?<count>\d+))? @@')
      if not ($m | is-empty) {
        let h = ($m | first)
        let start = ($h.start | into int)
        let count = if ($h.count | is-empty) { 1 } else { $h.count | into int }
        if $count > 0 {
          $rows = ($rows | append {file: $cur, start: $start, end: ($start + $count - 1)})
        }
      }
    }
  }
  $rows
}

# Whether any of the lines `f.line` to `f.end` changed. A hit with no `end` is
# one line. Only the test-free lints keep an `end`: their spans are expressions,
# and the call on the last line of a chain is what was written.
def in-ranges [f: record, ranges: list] {
  let last = ($f | get -o end | default $f.line)
  $ranges | any {|r| $r.file == $f.file and $last >= $r.start and $f.line <= $r.end }
}

# The changed ranges against `base`: the working tree and untracked `.rs` files,
# or with `committed` only `base...HEAD`.
def base-ranges [root: string, base: string, committed: bool] {
  let target = if $committed { $"($base)...HEAD" } else { $base }
  let diff = [
    "-c" "core.quotePath=false" "diff" "-U0" "--no-renames" "--no-color" "--relative"
    "--no-ext-diff" "--src-prefix=a/" "--dst-prefix=b/" $target "--"
  ]
  let shown = (argv-line (["git" "-C" "."] ++ $diff))
  let d = (^git -C $root ...$diff | complete)
  if $d.exit_code != 0 { return {command: $shown, error: $d.stderr, ranges: []} }
  let ranges = (changed-ranges $d.stdout)
  if $committed { return {command: $shown, error: null, ranges: $ranges} }
  let others = ["-c" "core.quotePath=false" "ls-files" "--others" "--exclude-standard"]
  let both = $"($shown) && (argv-line (['git' '-C' '.'] ++ $others))"
  let u = (^git -C $root ...$others | complete)
  if $u.exit_code != 0 { return {command: $both, error: $u.stderr, ranges: []} }
  let whole = (
    $u.stdout | lines | where {|f| $f | str ends-with ".rs" }
    | each {|f| {file: $f, start: 1, end: 2147483647} }
  )
  {command: $both, error: null, ranges: ($ranges ++ $whole)}
}

# --- writer mode -------------------------------------------------------------

# Clippy's restriction lints in the catalog. Writer mode does not turn these on;
# the crate does, in its lint tables, its source or rust-tops.yaml.
const RESTRICTION = [
  "clippy::allow_attributes" "clippy::allow_attributes_without_reason" "clippy::exhaustive_enums"
  "clippy::exhaustive_structs" "clippy::indexing_slicing" "clippy::iter_over_hash_type"
  "clippy::map_err_ignore" "clippy::string_add" "clippy::undocumented_unsafe_blocks"
  "clippy::unwrap_used"
]

# Lints that test code is free to hit.
const TEST_FREE = ["clippy::unwrap_used" "clippy::indexing_slicing"]

# `gates.lint.clippy_deny` of the nearest rust-tops.yaml, without `clippy::`.
def yaml-lints [start: string] {
  let file = (find-up $start "rust-tops.yaml")
  if $file == null { return [] }
  try { open $file | get -o gates.lint.clippy_deny | default [] | each {|l| $l | str replace "clippy::" "" } } catch { [] }
}

def is-test-path [file: string] {
  ($file | path split | drop 1 | any {|c| $c == "tests" }) or ($file | str ends-with "_test.rs")
}

# A line without its string and char literals and comments.
def code-only [text: string] {
  $text
  | str replace --all --regex '"(?:\\.|[^"\\])*"' '""'
  | str replace --all --regex "'(?:\\\\.|[^'\\\\])'" "' '"
  | str replace --all --regex '/\*.*?\*/' ''
  | str replace --regex '//.*' ''
}

# {start, end} line spans of the `#[cfg(test)]` and `#[test]` items in a file.
# Braces are counted; this only narrows the judgment list, never the findings.
def test-spans [lines: list] {
  mut spans = []
  mut start = 0
  mut depth = 0
  mut opened = false
  for r in ($lines | enumerate) {
    let n = $r.index + 1
    if $start == 0 {
      if not ($r.item =~ '^\s*#\[(?:cfg\((?:all\()?\s*test\b|test\])') { continue }
      $start = $n
      $depth = 0
      $opened = false
    }
    let code = (code-only $r.item)
    let opens = ($code | split chars | where {|c| $c == "{" } | length)
    let closes = ($code | split chars | where {|c| $c == "}" } | length)
    if $opens > 0 { $opened = true }
    $depth = $depth + $opens - $closes
    if ($opened and $depth <= 0) or ((not $opened) and ($code | str trim | str ends-with ";")) {
      $spans = ($spans | append {start: $start, end: $n})
      $start = 0
    }
  }
  if $start != 0 { $spans = ($spans | append {start: $start, end: ($lines | length)}) }
  $spans
}

# The changed lines of production code, as text.
def changed-code [root: string, ranges: list] {
  let files = ($ranges | get -o file | uniq | where {|f| ($f | str ends-with ".rs") and not (is-test-path $f) })
  $files
  | each {|f|
      let full = ($root | path join $f)
      if not ($full | path exists) { [] } else {
        let lines = (open --raw $full | lines)
        let spans = (test-spans $lines)
        let mine = ($ranges | where file == $f)
        $lines | enumerate
        | where {|r|
            let n = $r.index + 1
            ($mine | any {|m| $n >= $m.start and $n <= $m.end }) and not ($spans | any {|s| $n >= $s.start and $n <= $s.end })
          }
        | get -o item
      }
    }
  | flatten
}

# Which changed text makes a judgment rule worth asking. DOC rules read the
# line as written; the others read it without literals and comments. A rule
# with no row here is always asked.
const TRIGGERS = [
  {id: "API-4", re: '\.clone\(\)|\.to_owned\(\)'}
  {id: "API-5", re: '\bimpl\s+Into\s*<'}
  {id: "API-6", re: '->.*\bVec\s*<'}
  {id: "API-7", re: '\bpub\s'}
  {id: "API-16", re: '\bpub\s+(?:unsafe\s+)?trait\b'}
  {id: "API-17", re: '\bpub\s+(?:(?:const|async|unsafe)\s+)*(?:fn|struct|enum|trait|type|mod)\b'}
  {id: "CONC-3", re: '\b(?:Arc|Mutex|RwLock|RefCell|async)\b|\.lock\(\)|Guard\b'}
  {id: "DOC-5", re: '^\s*//[/!]'}
  {id: "ERR-5", re: 'Error\b'}
  {id: "ERR-6", re: 'Error\b|\bErr\(|\bmap_err\b|\b(?:bail|anyhow)!|\.context\('}
  {id: "IDIOM-10", re: '\bmatch\b|\.(?:map|and_then|filter|filter_map|fold|map_or|ok_or_else)\('}
  {id: "OWN-2", re: '\.clone\(\)'}
  {id: "PANIC-4", re: '[\w\)\]]\s+[-+*/%]\s+[\w\(]|[-+*/%]=|\bas\s+(?:u|i)(?:8|16|32|64|size)\b'}
  {id: "PANIC-5", re: '\.expect\(|\b(?:panic|assert|assert_eq|assert_ne|unreachable)!'}
  {id: "PANIC-6", re: '\.unwrap\(\)|\.expect\(|\b(?:panic|unreachable|todo|unimplemented)!|\bcatch_unwind\b'}
  {id: "PANIC-7", re: '\bDrop\b'}
  {id: "TEST-1", re: '\b(?:if|match|while)\b'}
  {id: "TEST-2", re: '\b(?:const|static)\s+[A-Z_]'}
  {id: "TRAIT-3", re: '\b(?:Ord|PartialOrd|PartialEq|Hash)\b'}
  {id: "TRAIT-4", re: '\bpub\s+(?:struct|enum)\b'}
  {id: "UNSAFE-5", re: '\bunsafe\b'}
]

def scoped-judgment [judgment: list, lines: list] {
  let code = ($lines | each {|l| code-only $l })
  $judgment
  | where {|j|
      let rows = ($TRIGGERS | where id == $j.id)
      if ($lines | is-empty) { false } else if ($rows | is-empty) { true } else {
        let re = ($rows | first | get re)
        let text = if ($j.id | str starts-with "DOC-") { $lines } else { $code }
        $text | any {|l| $l =~ $re }
      }
    }
}

# --- clippy run --------------------------------------------------------------

# {command, error, msgs} for one `cargo clippy` under `root`.
def clippy-run [root: string, targets: list, warn: list] {
  let args = (
    ["clippy"] ++ $targets ++ ["--all-features" "--message-format=json" "--" "--cap-lints" "warn"]
    ++ ($warn | each {|l| ["-W" $l] } | flatten)
  )
  let command = (argv-line (["cargo"] ++ $args))
  let run = (do { cd $root; ^cargo ...$args | complete })
  let msgs = (
    $run.stdout | lines
    | each {|l| try { $l | from json } catch { null } }
    | where {|m| ($m | describe | str starts-with "record") and ($m | get -o reason) == "compiler-message" }
  )
  if $run.exit_code == 0 { return {command: $command, error: null, msgs: $msgs} }
  let errs = ($msgs | where {|m| $m.message.level == "error" and not ($m.message.message | str starts-with "aborting") })
  let text = if ($errs | is-empty) {
    $run.stderr | str replace --all $root "."
  } else {
    $errs | first | get message.message
  }
  {command: $command, error: $"build failed: ($text)", msgs: []}
}

# --- commands ----------------------------------------------------------------

# No `--base`, or `--audit`: the whole tree, every catalog lint, every judgment
# rule. `--base <ref>` is writer mode: only what changed against `<ref>`.
def "main check" [--path: string = ".", --base: string, --committed-only, --audit] {
  let required = (required-nu $path)
  let found = (version | get version)
  if not (nu-ok $required $found) {
    return (fail "nu --version" $"nu ($required) required, found ($found)")
  }
  if $committed_only and $base == null {
    return (fail $SELF "--committed-only needs --base")
  }
  let writer = $base != null and not $audit
  if not (have cargo) { return (fail "cargo clippy" "cargo: command not found") }
  let rules = (try { open ($HERE | path join "rules.toml") | get rule } catch {|e| null })
  if $rules == null { return (fail $"open rules.toml" "rules.toml is missing or not a rule catalog") }
  let mine = ($rules | where {|r| "style" in $r.skills })
  let lints = ($mine | where kind == "lint" | sort-by check)
  let probes = ($mine | where kind == "probe" | sort-by id)
  let judgment = (
    $rules | where kind == "judgment" | where {|r| "style" in $r.skills } | sort-by id
    | each {|r| {id: $r.id, rule: $r.rule} }
  )

  let unknown = ($probes | where {|p| $p.check not-in $PROBES })
  if not ($unknown | is-empty) {
    return (fail "open rules.toml" $"rules.toml: unknown probe ($unknown | first | get check)")
  }

  let root = ($path | path expand)
  let opted = if $writer { yaml-lints $root } else { [] }
  let warn = (
    $lints | get check
    | where {|c| (not $writer) or ($c not-in $RESTRICTION) or ("restriction" in $opted) or (($c | str replace "clippy::" "") in $opted) }
  )
  mut command = "cargo metadata --no-deps --format-version 1"
  let meta = (do { cd $root; ^cargo metadata --no-deps --format-version 1 | complete })
  if $meta.exit_code != 0 {
    return (fail "cargo metadata --no-deps --format-version 1" $"build failed: ($meta.stderr | str replace --all $root '.')")
  }
  let ws = ($meta.stdout | from json | get workspace_root | path expand)
  let all = (clippy-run $root ["--all-targets"] $warn)
  $command = $"($command) && ($all.command)"
  if $all.error != null { return (fail $command $all.error) }
  let from_lints = (lint-findings $all.msgs $lints $ws $root)
  mut from_probes = []
  let files = (rust-files $root)
  for f in $files {
    let ls = (open --raw ($root | path join $f) | lines)
    let rows = (
      $ls | enumerate
      | each {|r| {file: $f, n: ($r.index + 1), text: $r.item, prev: (if $r.index == 0 { "" } else { $ls | get ($r.index - 1) })} }
    )
    for p in $probes {
      let hits = (run-probe $p.check $rows)
      $from_probes = ($from_probes | append ($hits | each {|h|
        {rule: $p.id, kind: "probe", file: $f, line: $h.line, col: $h.col, message: $h.message, fix: $p.fix}
      }))
    }
  }
  mut findings = ($from_lints ++ $from_probes | uniq-by file line col rule | sort-by file line col rule)

  mut asked = $judgment
  # Every other hit counts by the line it starts on: a whole item is its span.
  let free = ($lints | where check in $TEST_FREE | get id)
  $findings = ($findings | each {|f| if $f.rule in $free { $f } else { $f | upsert end $f.line } })
  if $writer {
    let changed = (base-ranges $root $base $committed_only)
    $command = $"($command) && ($changed.command)"
    if $changed.error != null {
      return (fail $command ($changed.error | str replace --all $root "."))
    }
    $findings = ($findings | where {|f| in-ranges $f $changed.ranges })
    # A hit on a test-free lint stays only if a build without tests has it too.
    $findings = ($findings | where {|f| not ($f.rule in $free and (is-test-path $f.file)) })
    if ($findings | any {|f| $f.rule in $free }) {
      let product = (clippy-run $root [] $warn)
      $command = $"($command) && ($product.command)"
      if $product.error != null { return (fail $command $product.error) }
      let real = (lint-findings $product.msgs $lints $ws $root | each {|f| $"($f.rule) ($f.file):($f.line):($f.col)" })
      $findings = ($findings | where {|f| $f.rule not-in $free or $"($f.rule) ($f.file):($f.line):($f.col)" in $real })
    }
    $asked = (scoped-judgment $judgment (changed-code $root $changed.ranges))
  }

  $findings = ($findings | each {|f| $f | reject -o end })

  {
    ok: ($findings | is-empty)
    command: $command
    data: {
      base: (if $writer { $base } else { null })
      findings: $findings
      judgment: $asked
    }
  } | to json
}

def main [] {
  fail $SELF "pass check"
}
