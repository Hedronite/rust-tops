#!/usr/bin/env nu
# Regenerate conformance/tops_core.json from the Bend spec.
#
# Runs conformance/gen.bend (which imports spec/tops_core.bend and prints one
# flat line per case), maps the Row constructors to the Rust strings, and
# writes the JSON, sorted, one case per line. It writes only that one file.
#
# Needs `bend` on PATH. Set HOME to a scratch directory and BEND_NO_TELEMETRY=1.

const ROWS = {
  RowPass: "pass"
  RowFail: "fail"
  RowToolMissing: "tool-missing"
  RowToolMismatch: "tool-mismatch"
  RowVacuous: "vacuous"
  RowNotRun: "not-run"
}

def row [name: string] {
  if $name not-in ($ROWS | columns) {
    error make { msg: $"gen-conformance: unknown Row constructor ($name)" }
  }
  $ROWS | get $name
}

def flag [text: string] {
  match $text {
    "True" => true
    "False" => false
    _ => { error make { msg: $"gen-conformance: not a Bool: ($text)" } }
  }
}

def outcomes [text: string] {
  $text | split row "," | where { |it| $it != "" }
}

def main [] {
  let root = ($env.FILE_PWD | path dirname)
  let out = ($root | path join conformance tops_core.json)

  let ran = (^bend ($root | path join conformance gen.bend) | complete)
  if $ran.exit_code != 0 {
    error make { msg: $"gen-conformance: bend failed (exit ($ran.exit_code)): ($ran.stderr)" }
  }
  let version = (^bend version | str trim | split row " " | last)

  let cases = ($ran.stdout | lines | where { |it| $it != "" } | each { |line|
    let f = ($line | split row "|")
    match $f.0 {
      "runs_verdict" => {
        { fn: "runs_verdict", input: { runs: (outcomes $f.1) }, expect: (row $f.2) }
      }
      "layer_verdict" => {
        { fn: "layer_verdict", input: { runs: (outcomes $f.1), missed: (flag $f.2) }, expect: (row $f.3) }
      }
      "live" => {
        let day = if $f.1 == "Invalid" { null } else { $f.1 | into int }
        { fn: "live", input: { day: $day, today: ($f.2 | into int) }, expect: (flag $f.3) }
      }
      _ => { error make { msg: $"gen-conformance: unknown line: ($line)" } }
    }
  })

  let body = ($cases
    | each { |c| $c | insert key ($c.input | to json -r) }
    | sort-by fn key
    | each { |c| $c | reject key | to json -r }
    | str join ",\n")

  let head = ({ spec: "spec/tops_core.bend", bend_version: $version } | to json -r | str replace --regex '\}$' ',"cases":[')
  [$head "\n" $body "\n]}\n"] | str join | save --force $out
  print $"gen-conformance: ($cases | length) cases -> conformance/tops_core.json"
}
