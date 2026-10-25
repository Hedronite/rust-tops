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
  let command = (argv-line (["git"] ++ $args))
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

def main [] {
  {ok: false, command: "git diff", error: "pass files or diff", exit: 2} | to json
}
