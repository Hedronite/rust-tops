#!/usr/bin/env nu
# Wraps `cargo tops gate`. Does not re-implement the gate.
# The report's top-level pass bit only means bend exited and its verdicts parsed.
# This script never reads that field. `ok` is true only when the command exits 0
# and every law row is checked with verdict Allow or Ack.

def have [bin: string] {
  not ((which $bin | where type == external) | is-empty)
}

def quote-arg [a: string] {
  if ($a | str contains " ") { $"\"($a)\"" } else { $a }
}

def argv-line [args: list<string>] {
  $args | each {|a| quote-arg $a } | str join " "
}

def clip [text: string] {
  $text | str trim | str substring 0..<400
}

def remedy-problem [text: string] {
  if ($text | str contains "not on PATH") or ($text | str contains "command not found") {
    "install it. cargo-tops does not vendor engines. A missing tool is a fail, not a skip."
  } else if ($text | str starts-with "coverage:") {
    "add a test for the specified missing branch, not a tautology. Empty LCOV: the recorder never ran; fix the kit, do not skip."
  } else if ($text | str starts-with "crap:") {
    "reduce real branching or cover the risky arm. Do not extract foo_part_2. Empty CRAP JSON: the recorder never ran; fix the kit, do not skip."
  } else if ($text | str starts-with "mutation:") {
    "assert the behavior the mutant broke. Never --baseline skip."
  } else if ($text | str starts-with "laws:") {
    "read that law in rust-tops/cases.json and fix the fact it read. Do not edit the pinned kit files."
  } else if ($text | str contains "ENOENT") {
    "honor CARGO_BIN_EXE_* / $CARGO_TARGET_DIR / one-level target/ scan."
  } else if ($text | str contains "callgrind") or ($text | str contains "perf") {
    "profile, then algorithm / alloc / layout. unsafe is last and Miri-clean."
  } else {
    "Fix the failing command against the contract. Do not generate a test from the implementation."
  }
}

def remedy-law [status: string, verdict: string] {
  if $status == "not-evaluated" {
    "the law was not checked. It is not a pass. Read reason in cases.json. If the runner has no fact for that law, leave it. Do not edit the pinned kit files and do not encode it as Allow."
  } else if $verdict == "Refuse" {
    "read that law in rust-tops/cases.json and fix the fact it read. Do not edit the pinned kit files."
  } else if $verdict == "unreported" {
    "unreported is not a pass. bend stops at the first refusal. Read rust-tops/cases.json."
  } else {
    "the law is not a passing checked verdict. Read rust-tops/cases.json. Do not count it as a pass."
  }
}

export def gate-verdict [exit_code: int, cases: any] {
  if $cases == null {
    let why = if $exit_code == 0 {
      "cases.json is missing or not JSON"
    } else {
      $"cargo tops gate exited ($exit_code) and cases.json is missing or not JSON"
    }
    return {ok: false, error: $why, fixes: []}
  }
  let laws = ($cases.laws? | default null)
  if $laws == null {
    return {ok: false, error: "cases.json has no laws array", fixes: []}
  }
  let laws_kind = ($laws | describe)
  let laws_ok = ($laws_kind | str starts-with "list") or ($laws_kind | str starts-with "table")
  if not $laws_ok {
    return {ok: false, error: "cases.json laws is not a list", fixes: []}
  }
  if ($laws | is-empty) {
    return {ok: false, error: "cases.json listed no laws", fixes: []}
  }
  let fixes = (
    $laws | each {|row|
      let status = ($row.status? | default "")
      let verdict = ($row.verdict? | default "")
      if $status == "checked" and $verdict in ["Allow" "Ack"] {
        null
      } else {
        {
          law: ($row.law? | default "")
          status: $status
          verdict: $verdict
          reason: ($row.reason? | default "")
          remedy: (remedy-law $status $verdict)
        }
      }
    } | where {|row| $row != null}
  )
  let n_fix = ($fixes | length)
  let unevaluated = ($fixes | where {|row| $row.status == "not-evaluated"} | length)
  let all_clear = ($exit_code == 0) and ($fixes | is-empty)
  let error = if $all_clear {
    ""
  } else if $exit_code != 0 and ($fixes | is-empty) {
    $"cargo tops gate exited ($exit_code)"
  } else {
    $"failing laws: ($n_fix), not-evaluated: ($unevaluated)"
  }
  {ok: $all_clear, error: $error, fixes: $fixes}
}

def cargo-target [path: string] {
  let from_env = ($env.CARGO_TARGET_DIR? | default "" | str trim)
  if not ($from_env | is-empty) {
    return $from_env
  }
  if not (have cargo) {
    error make {msg: "cargo: command not found"}
  }
  let manifest = ($path | path join "Cargo.toml")
  let r = (^cargo metadata --format-version 1 --no-deps --manifest-path $manifest | complete)
  if $r.exit_code != 0 {
    error make {msg: (clip $"cargo metadata failed, exit ($r.exit_code): ($r.stderr)")}
  }
  let parsed = (try { $r.stdout | from json } catch {|err| error make {msg: $"cargo metadata did not return JSON: ($err.msg)"}})
  let kind = ($parsed | describe)
  if not ($kind | str starts-with "record") {
    error make {msg: "cargo metadata did not return JSON"}
  }
  let dir = ($parsed.target_directory? | default "" | str trim)
  if ($dir | is-empty) {
    error make {msg: "cargo metadata JSON has no target_directory"}
  }
  $dir
}

def read-cases [cases: string, started: datetime] {
  if not ($cases | path exists) {
    return {state: "missing", cases: null}
  }
  let modified = (ls -f $cases | get modified | first)
  if $modified < ($started - 2sec) {
    return {state: "stale", cases: null}
  }
  let parsed = (try { open $cases } catch { null })
  if $parsed == null {
    return {state: "bad", cases: null}
  }
  let kind = ($parsed | describe)
  if not ($kind | str starts-with "record") {
    return {state: "bad", cases: null}
  }
  {state: "ok", cases: $parsed}
}

def summary-line [text: string] {
  let rows = ($text | lines | where {|row|
    ($row | str starts-with "ok: ") or ($row | str starts-with "fail: ") or ($row | str starts-with "dry-run: ")
  })
  if ($rows | is-empty) { "" } else { $rows | last }
}

def problem-lines [text: string] {
  $text | lines | each {|row| $row | str trim } | where {|row| $row | str starts-with "- " } | each {|row| $row | str substring 2.. }
}

def layer-rows [text: string, problems: list] {
  $text | lines | each {|row|
    let trimmed = ($row | str trim)
    if not ($trimmed | str starts-with "==> [") {
      null
    } else {
      let rest = ($trimmed | str substring 5..)
      let parts = ($rest | split row "] ")
      if ($parts | length) < 2 {
        null
      } else {
        let id = ($parts | first)
        let command = ($parts | skip 1 | str join "] ")
        let mine = ($problems | where {|p| $p | str starts-with $"($id):" })
        let named = ($mine | any {|p| $p | str contains $command })
        let unnamed = (not ($mine | is-empty)) and ($mine | all {|p| not ($p | str contains "`") })
        let failed = $named or $unnamed
        {
          id: $id
          command: $command
          ok: (not $failed)
          remedy: (if $failed { remedy-problem ($mine | first) } else { "" })
        }
      }
    }
  } | where {|row| $row != null }
}

def main [
  --path: string = "."
  --max-tier: int = 1
] {
  let meta_cmd = (argv-line ["cargo" "metadata" "--format-version" "1" "--no-deps" "--manifest-path" ($path | path join "Cargo.toml")])
  let located = (try { {ok: true, dir: (cargo-target $path)} } catch {|err| {ok: false, error: $err.msg}})
  if not $located.ok {
    return ({ok: false, command: $meta_cmd, error: (clip $located.error), exit: 2} | to json)
  }
  let cases = ($located.dir | path join "rust-tops" "cases.json")
  let started = (date now)
  ^rm -f $cases
  let args = ["tops" "gate" "--path" $path "--max-tier" ($max_tier | into string)]
  let command = (argv-line (["cargo"] ++ $args))
  if not (have cargo) {
    return ({ok: false, command: $command, error: "cargo: command not found", exit: 127} | to json)
  }
  let r = (^cargo ...$args | complete)
  let text = ([$r.stdout $r.stderr] | str join "\n")
  let summary = (summary-line $text)
  let problems = (problem-lines $text | each {|p| {text: $p, remedy: (remedy-problem $p)}})
  let layers = (layer-rows $text ($problems | get text))
  let loaded = (read-cases $cases $started)
  let cases_value = if $loaded.state == "ok" { $loaded.cases } else { null }
  let verdict = (gate-verdict $r.exit_code $cases_value)
  mut error = $verdict.error
  if $loaded.state == "stale" {
    $error = "cases.json is older than this run"
  } else if $loaded.state == "bad" or $loaded.state == "missing" {
    $error = "cases.json is missing or not JSON"
  } else if $r.exit_code != 0 and not ($summary | is-empty) {
    $error = (if ($verdict.fixes | is-empty) { $summary } else { $"($summary); ($verdict.error)" })
  }
  {
    ok: ($verdict.ok and $loaded.state == "ok")
    command: $command
    exit: $r.exit_code
    error: (if ($verdict.ok and $loaded.state == "ok") { "" } else { clip $error })
    data: {
      summary: $summary
      problems: $problems
      layers: $layers
      fixes: $verdict.fixes
      laws: (if $loaded.state == "ok" { $loaded.cases.laws? | default [] } else { [] })
    }
  } | to json
}
