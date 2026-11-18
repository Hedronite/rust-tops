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

def minor [v: string] {
  $v | split row "." | first 2 | str join "."
}

# Nearest rust-tops.lock at or above `start`, or null.
def find-lock [start: string] {
  mut dir = ($start | path expand)
  loop {
    let cand = ($dir | path join "rust-tops.lock")
    if ($cand | path exists) { return $cand }
    let parent = ($dir | path dirname)
    if $parent == $dir { return null }
    $dir = $parent
  }
}

def required-nu [start: string] {
  let lock = (find-lock $start)
  if $lock == null { return $DEFAULT_NU }
  let doc = (try { open --raw $lock | from toml } catch { {} })
  $doc | get -o tools.nushell | default $DEFAULT_NU
}

# --- probes -----------------------------------------------------------------
# A probe takes one file as a list of {n, text} rows (n is the 1-based line) and
# returns {line, col, message} rows. Builtins only.

def is-comment [text: string] {
  $text | str trim --left | str starts-with "//"
}

# 1-based column, counted in Unicode scalars as clippy counts it.
def col-of [text: string, needle: string] {
  let b = ($text | str index-of --utf-8-bytes $needle)
  1 + ($text | encode utf8 | bytes at 0..<$b | decode utf-8 | str length --chars)
}

# Rows whose code part (not a `//` line) holds `needle`.
def code-hits [rows: list, needle: string, message: string] {
  $rows
  | where {|r| (not (is-comment $r.text)) and ($r.text | str contains $needle) }
  | each {|r| {line: $r.n, col: (col-of $r.text $needle), message: $message} }
}

def probe-process-exit [rows: list] {
  code-hits $rows "process::exit(" "process::exit skips destructors"
}

def probe-todo-comment [rows: list] {
  $rows
  | where {|r| (is-comment $r.text) and ($r.text | str contains "TODO") and (not ($r.text | str contains "TODO(#")) }
  | each {|r| {line: $r.n, col: (col-of $r.text "TODO"), message: "TODO has no issue number"} }
}

def probe-box-dyn-error [rows: list] {
  $rows
  | where {|r| (not (is-comment $r.text)) and ($r.text | str contains "pub fn") and (($r.text | str contains "Box<dyn Error") or ($r.text | str contains "Box<dyn std::error::Error")) }
  | each {|r| {line: $r.n, col: (col-of $r.text "Box<dyn"), message: "public fn returns a boxed error"} }
}

const PROBES = ["box-dyn-error" "process-exit" "todo-comment"]

def run-probe [name: string, rows: list] {
  match $name {
    "process-exit" => (probe-process-exit $rows)
    "todo-comment" => (probe-todo-comment $rows)
    "box-dyn-error" => (probe-box-dyn-error $rows)
    _ => []
  }
}

def rust-files [root: string] {
  glob ($root | path join "**/*.rs")
  | each {|f| $f | path relative-to $root }
  | where {|rel| not ($rel | path split | any {|c| $c in ["target" ".git" "node_modules"] }) }
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
      let p = ($l | str substring 4..)
      $cur = if $p == "/dev/null" { "" } else if ($p | str starts-with "b/") { $p | str substring 2.. } else { $p }
    } else if $cur != "" and ($l | str starts-with "@@ ") {
      let m = ($l | parse --regex '^@@ -\d+(?:,\d+)? \+(?<start>\d+)(?:,(?<count>\d+))? @@')
      if not ($m | is-empty) {
        let h = ($m | first)
        let start = ($h.start | into int)
        let count = if $h.count == "" { 1 } else { $h.count | into int }
        if $count > 0 {
          $rows = ($rows | append {file: $cur, start: $start, end: ($start + $count - 1)})
        }
      }
    }
  }
  $rows
}

def in-ranges [f: record, ranges: list] {
  $ranges | any {|r| $r.file == $f.file and $f.line >= $r.start and $f.line <= $r.end }
}

# --- commands ----------------------------------------------------------------

def "main check" [--path: string = ".", --base: string] {
  let required = (required-nu $path)
  let found = (version | get version)
  if (minor $required) != (minor $found) {
    return (fail "nu --version" $"nu ($required) required, found ($found)")
  }
  if not (have cargo) { return (fail "cargo clippy" "cargo: command not found") }
  let rules = (try { open ($HERE | path join "rules.toml") | get rule } catch {|e| null })
  if $rules == null { return (fail $"open rules.toml" "rules.toml is missing or not a rule catalog") }
  let lints = ($rules | where kind == "lint" | sort-by check)
  let probes = ($rules | where kind == "probe" | sort-by id)
  let judgment = (
    $rules | where kind == "judgment" | where {|r| "style" in $r.skills } | sort-by id
    | each {|r| {id: $r.id, rule: $r.rule} }
  )

  let unknown = ($probes | where {|p| $p.check not-in $PROBES })
  if not ($unknown | is-empty) {
    return (fail "open rules.toml" $"rules.toml: unknown probe ($unknown | first | get check)")
  }

  let root = ($path | path expand)
  let args = (
    ["clippy" "--all-targets" "--all-features" "--message-format=json" "--"]
    ++ ($lints | get check | each {|l| ["-W" $l] } | flatten)
  )
  mut command = (argv-line (["cargo"] ++ $args))
  let meta = (do { cd $root; ^cargo metadata --no-deps --format-version 1 | complete })
  if $meta.exit_code != 0 {
    return (fail "cargo metadata --no-deps --format-version 1" $"build failed: ($meta.stderr | str replace --all $root '.')")
  }
  let ws = ($meta.stdout | from json | get workspace_root | path expand)
  let run = (do { cd $root; ^cargo ...$args | complete })
  let msgs = (
    $run.stdout | lines
    | each {|l| try { $l | from json } catch { null } }
    | where {|m| ($m | describe | str starts-with "record") and ($m | get -o reason) == "compiler-message" }
  )
  if $run.exit_code != 0 {
    let errs = ($msgs | where {|m| $m.message.level == "error" and not ($m.message.message | str starts-with "aborting") })
    let text = if ($errs | is-empty) {
      $run.stderr | str replace --all $root "."
    } else {
      $errs | first | get message.message
    }
    return (fail $command $"build failed: ($text)")
  }

  let from_lints = (lint-findings $msgs $lints $ws $root)
  mut from_probes = []
  let files = (rust-files $root)
  for f in $files {
    let rows = (open --raw ($root | path join $f) | lines | enumerate | each {|r| {n: ($r.index + 1), text: $r.item} })
    for p in $probes {
      let hits = (run-probe $p.check $rows)
      $from_probes = ($from_probes | append ($hits | each {|h|
        {rule: $p.id, kind: "probe", file: $f, line: $h.line, col: $h.col, message: $h.message, fix: $p.fix}
      }))
    }
  }
  mut findings = ($from_lints ++ $from_probes | uniq-by file line col rule | sort-by file line col rule)

  if $base != null {
    let d = (^git -C $root -c core.quotePath=false diff -U0 --no-renames --no-color --relative $"($base)...HEAD" | complete)
    $command = $"($command) && (argv-line ['git' '-C' '.' 'diff' '-U0' $'($base)...HEAD'])"
    if $d.exit_code != 0 {
      return (fail $command ($d.stderr | str replace --all $root "."))
    }
    let ranges = (changed-ranges $d.stdout)
    $findings = ($findings | where {|f| in-ranges $f $ranges })
  }

  {
    ok: ($findings | is-empty)
    command: $command
    data: {
      base: $base
      findings: $findings
      judgment: $judgment
    }
  } | to json
}

def main [] {
  fail $SELF "pass check"
}
