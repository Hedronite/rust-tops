#!/usr/bin/env nu
# Wraps `git diff` for the review gate in docs/REVIEW.md. Does not emit a verdict.

const RULES = [
  "The reviewer did not write the diff."
  "Write the inputs you will run before reading the implementation."
  "Run those inputs. Compare results with the base commit."
  "Say what you executed. A green test suite is not evidence."
  "High-risk changes (business rules, thresholds, scoring, pricing, or a validation gate) get a second reviewer on a different model."
  "Review every fix commit again."
  "Each finding is BLOCKING or NIT, with input, expected, and actual."
  "Report N of the K found so far."
  "End with exactly one line: VERDICT: APPROVE or VERDICT: CHANGES."
  "Follow docs/REVIEW.md. This script does not emit a verdict."
]

def have [bin: string] {
  not ((which $bin | where type == external) | is-empty)
}

def quote-arg [a: string] {
  if ($a | str contains " ") { $"\"($a)\"" } else { $a }
}

def argv-line [args: list<string>] {
  $args | each {|a| quote-arg $a } | str join " "
}

def git-run [args: list<string>] {
  # The command shown holds paths relative to --path, so `-C <path>` reads `-C .`.
  let shown = if ($args | first) == "-C" { ["-C" "."] ++ ($args | skip 2) } else { $args }
  let command = (argv-line (["git"] ++ $shown))
  if not (have git) {
    return {ok: false, command: $command, error: "git: command not found", exit: 127}
  }
  let r = (^git ...$args | complete)
  if $r.exit_code == 0 {
    {ok: true, command: $command, exit: 0, stdout: $r.stdout}
  } else {
    let msg = ([$r.stderr $r.stdout] | str join "\n" | str trim)
    {ok: false, command: $command, error: ($msg | str substring 0..<400), exit: $r.exit_code}
  }
}

def names [text: string] {
  $text | lines | each {|row| $row | str trim } | where {|row| not ($row | is-empty)}
}

# Default range is commits on `base...HEAD` plus the working tree:
# `git diff HEAD` (staged and unstaged tracked files) and untracked names.
# `--committed-only` drops the working tree.
def "main files" [--base: string = "origin/main", --path: string = ".", --committed-only] {
  let committed = (git-run ["-C" $path "diff" "--name-only" $"($base)...HEAD"])
  if not $committed.ok { return ($committed | to json) }
  let committed_names = (names $committed.stdout)
  mut work_names = []
  mut untracked = []
  mut command = $committed.command
  if not $committed_only {
    let work = (git-run ["-C" $path "diff" "--name-only" "HEAD"])
    if not $work.ok { return ($work | to json) }
    $work_names = (names $work.stdout)
    let extra = (git-run ["-C" $path "ls-files" "--others" "--exclude-standard"])
    if not $extra.ok { return ($extra | to json) }
    $untracked = (names $extra.stdout)
    $command = $"($committed.command) && ($work.command) && ($extra.command)"
  }
  let files = ($committed_names ++ $work_names ++ $untracked | sort | uniq)
  {
    ok: true
    command: $command
    data: {
      base: $base
      includes_worktree: (not $committed_only)
      files: $files
      committed: $committed_names
      uncommitted: ($work_names ++ $untracked | sort | uniq)
      untracked: $untracked
      read_diff: false
      doc: "docs/REVIEW.md"
      next: "Write the inputs you will run. Then run: nu rust-tops-review.nu diff"
      rules: $RULES
    }
  } | to json
}

def "main diff" [--base: string = "origin/main", --path: string = ".", --committed-only] {
  let committed = (git-run ["-C" $path "diff" $"($base)...HEAD"])
  if not $committed.ok { return ($committed | to json) }
  mut patch = $committed.stdout
  mut command = $committed.command
  mut untracked = []
  if not $committed_only {
    let work = (git-run ["-C" $path "diff" "HEAD"])
    if not $work.ok { return ($work | to json) }
    if not ($work.stdout | str trim | is-empty) {
      $patch = $"($patch)($work.stdout)"
    }
    let extra = (git-run ["-C" $path "ls-files" "--others" "--exclude-standard"])
    if not $extra.ok { return ($extra | to json) }
    $untracked = (names $extra.stdout)
    $command = $"($committed.command) && ($work.command) && ($extra.command)"
  }
  {
    ok: true
    command: $command
    data: {
      base: $base
      includes_worktree: (not $committed_only)
      read_diff: true
      untracked: $untracked
      diff: $patch
      doc: "docs/REVIEW.md"
      rules: $RULES
    }
  } | to json
}

# --- style and answers --------------------------------------------------------
# `style` runs the style skill's `check` and keeps the findings on lines changed
# in the review range. `answers` checks that every review judgment ID was
# answered. Neither prints a verdict. Same input gives the same bytes.

const HERE = (path self | path dirname)
const DEFAULT_NU = "0.108.0"
const STATUSES = ["ok" "finding" "n/a"]

def fail [command: string, error: string] {
  {ok: false, command: $command, error: ($error | str trim | str substring 0..<400)} | to json
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

# The error text when this nu cannot stand in for the pinned one, else null.
def nu-mismatch [start: string] {
  let lock = (find-lock $start)
  let doc = if $lock == null { {} } else { try { open --raw $lock | from toml } catch { {} } }
  let required = ($doc | get -o tools.nushell | default $DEFAULT_NU)
  let found = (version | get version)
  if (nu-ok $required $found) { null } else { $"nu ($required) required, found ($found)" }
}

def style-dir [] {
  $HERE | path dirname | path join "rust-tops-style"
}

def catalog [] {
  try { open (style-dir | path join "rules.toml") | get rule } catch { null }
}

# Rows {file, start, end} for the lines a unified diff adds or changes.
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

def in-ranges [f: record, ranges: list] {
  $ranges | any {|r| $r.file == $f.file and $f.line >= $r.start and $f.line <= $r.end }
}

# `--committed-only` takes line numbers from HEAD, but clippy and the probes read
# the working tree, so tracked edits would move hits off their lines. Untracked
# files cannot. {command, error}: `error` is null for a clean tree.
const CLEAN_ARGS = ["status" "--porcelain" "--untracked-files=no"]
const DIRTY = "--committed-only needs a clean tree: commit or stash the tracked changes, or drop --committed-only"

def clean-tree [root: string] {
  let command = (["git" "-C" "."] ++ $CLEAN_ARGS | str join " ")
  let r = (^git -C $root ...$CLEAN_ARGS | complete)
  let error = if $r.exit_code != 0 {
    $r.stderr | str replace --all ($root | path expand) "."
  } else if not ($r.stdout | str trim | is-empty) { $DIRTY } else { null }
  {command: $command, error: $error}
}

def "main style" [--base: string = "origin/main", --path: string = ".", --committed-only] {
  let own = "nu rust-tops-review.nu style"
  let wrong = (nu-mismatch $path)
  if $wrong != null { return (fail "nu --version" $wrong) }
  let rules = (catalog)
  if $rules == null { return (fail "open rules.toml" "rules.toml is missing or not a rule catalog") }
  let script = (style-dir | path join "rust-tops-style.nu")
  if not ($script | path exists) { return (fail $own "rust-tops-style.nu is missing beside this skill") }

  mut checked = ""
  if $committed_only {
    let tree = (clean-tree $path)
    if $tree.error != null { return (fail $tree.command $tree.error) }
    $checked = $"($tree.command) && "
  }

  let child = (^$nu.current-exe $script check --path $path | complete)
  let doc = (try { $child.stdout | from json } catch { null })
  if $doc == null { return (fail $"nu rust-tops-style.nu check" $"the style check printed no JSON: ($child.stderr)") }
  if ($doc | get -o error) != null { return $child.stdout }

  let committed = (git-run ["-C" $path "-c" "core.quotePath=false" "diff" "-U0" "--no-renames" "--no-color" "--relative" $"($base)...HEAD"])
  if not $committed.ok { return (fail $"git -C . diff -U0 ($base)...HEAD" $committed.error) }
  mut command = $"($checked)($doc.command) && git -C . diff -U0 ($base)...HEAD"
  mut ranges = (changed-ranges $committed.stdout)
  if not $committed_only {
    let work = (git-run ["-C" $path "-c" "core.quotePath=false" "diff" "-U0" "--no-renames" "--no-color" "--relative" "HEAD"])
    if not $work.ok { return (fail "git -C . diff -U0 HEAD" $work.error) }
    let extra = (git-run ["-C" $path "ls-files" "--others" "--exclude-standard"])
    if not $extra.ok { return (fail "git -C . ls-files --others --exclude-standard" $extra.error) }
    $ranges = ($ranges ++ (changed-ranges $work.stdout) ++ (names $extra.stdout | each {|f| {file: $f, start: 1, end: 2147483647} }))
    $command = $"($command) && git -C . diff -U0 HEAD && git -C . ls-files --others --exclude-standard"
  }
  let findings = ($doc.data.findings | where {|f| in-ranges $f $ranges })
  let judgment = (
    $rules | where kind == "judgment" | where {|r| "review" in $r.skills } | sort-by id
    | each {|r| {id: $r.id, rule: $r.rule} }
  )
  {
    ok: ($findings | is-empty)
    command: $command
    data: {
      base: $base
      includes_worktree: (not $committed_only)
      findings: $findings
      judgment: $judgment
    }
  } | to json
}

# `<file>` is TOML: [answers] "<ID>" = { status = "ok" | "finding" | "n/a", note = "..." }
def "main answers" [file: string] {
  let shown = (try { $file | path expand | path relative-to (pwd) } catch { $file })
  let own = $"nu rust-tops-review.nu answers ($shown)"
  let wrong = (nu-mismatch ($file | path expand | path dirname))
  if $wrong != null { return (fail "nu --version" $wrong) }
  let rules = (catalog)
  if $rules == null { return (fail "open rules.toml" "rules.toml is missing or not a rule catalog") }
  let required = ($rules | where kind == "judgment" | where {|r| "review" in $r.skills } | get id | sort)
  let doc = (try { open --raw $file | from toml } catch { null })
  if $doc == null { return (fail $own "the answers file is missing or not TOML") }
  let answers = ($doc | get -o answers | default {})
  if not (($answers | describe) | str starts-with "record") { return (fail $own "[answers] must be a table") }
  let given = ($answers | columns | sort)
  mut problems = []
  for id in $required {
    if $id not-in $given { $problems = ($problems | append {id: $id, problem: "missing"}) }
  }
  for id in $given {
    if $id not-in $required {
      $problems = ($problems | append {id: $id, problem: "unknown"})
    } else {
      let e = ($answers | get $id)
      if not (($e | describe) | str starts-with "record") {
        $problems = ($problems | append {id: $id, problem: "bad entry"})
      } else if ($e | get -o status) not-in $STATUSES {
        $problems = ($problems | append {id: $id, problem: "bad status"})
      } else if ($e | get status) == "finding" and (($e | get -o note | default "" | into string | str trim) | is-empty) {
        $problems = ($problems | append {id: $id, problem: "empty note"})
      }
    }
  }
  let sorted = ($problems | sort-by id problem)
  {
    ok: ($sorted | is-empty)
    command: $own
    data: {
      required: $required
      problems: $sorted
    }
  } | to json
}

def main [] {
  {ok: false, command: "nu rust-tops-review.nu", error: "pass files, diff, style or answers", exit: 2} | to json
}
