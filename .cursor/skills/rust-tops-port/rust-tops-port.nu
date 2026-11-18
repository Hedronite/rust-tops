#!/usr/bin/env nu
# MIT. Distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. Copyright (c) 2026 Morgan Linton. See LICENSES/BeautifulRust-MIT.txt.
#
# Test inventory and port verification for a port to Rust.
#   inventory  lists the original tests with the fixed patterns below
#   verify     checks that every original test maps to a Rust test that exists and passes
# Output is one JSON object: ok, command, and data or error. It never prints a verdict.
# The same input gives the same bytes: lists are sorted, paths are relative, no time or host.
#
# Fixed patterns per language. A file is read only when its extension matches.
#   python  *.py                     a line `def test_<name>(` or `async def test_<name>(`
#   c       *.c *.h                  under a `tests` directory: `TEST(<suite>, <name>)`,
#   cpp     *.c *.h *.cc *.cpp         `TEST_F(<suite>, <name>)`, or a function `test_<name>(`
#                                      with a void, int or bool return type
#   go      *_test.go                a line `func Test<Name>(`
#   js      *.js *.jsx *.mjs *.cjs   a line holding `test(` or `it(` followed by a quoted title
#   ts      *.ts *.tsx *.mts *.cts   (same as js)
# A test id is `<path relative to --from>::<name>`. A repeated JS title gets a `#2`, `#3` suffix.
# Directories named node_modules, target, .git, venv, .venv and __pycache__ are skipped.

const HERE = (path self | path dirname)
const LANGS = ["c" "cpp" "python" "go" "js" "ts"]
const SKIP_DIRS = ["node_modules" "target" ".git" "venv" ".venv" "__pycache__"]
const DEFAULT_NU = "0.108.0"

def fail [command: string, error: string] {
  {ok: false, command: $command, error: $error} | to json
}

def argv [parts: list<string>] {
  $parts | each {|a| if ($a | str contains " ") { $"\"($a)\"" } else { $a } } | str join " "
}

# The catalog entry for a rule: its text and fix. Read from the style skill's rules.toml.
def catalog [] {
  let candidates = [
    ($HERE | path join ".." "rust-tops-style" "rules.toml")
    ($HERE | path join "rules.toml")
  ]
  let found = ($candidates | where {|p| $p | path exists })
  if ($found | is-empty) { return null }
  open --raw ($found | first) | from toml | get rule
}

# The pinned Nushell version as written, from the nearest rust-tops.lock at or above `start`.
def pinned-nu [start: string] {
  mut dir = ($start | path expand)
  loop {
    let lock = ($dir | path join "rust-tops.lock")
    if ($lock | path exists) {
      let hit = (open --raw $lock | lines | parse --regex '^\s*nushell\s*=\s*"(?P<v>[0-9]+\.[0-9]+[^"]*)"')
      if not ($hit | is-empty) { return {v: ($hit | first | get v)} }
    }
    let up = ($dir | path dirname)
    if $up == $dir { break }
    $dir = $up
  }
  {v: $DEFAULT_NU}
}

def minor [v: string] {
  $v | split row "." | first 2 | str join "."
}

def check-nu [start: string, command: string] {
  let want = (pinned-nu $start | get v)
  let have = (version).version
  if (minor $want) != (minor $have) {
    return (fail $command $"nu ($want) required, found ($have)")
  }
  null
}

def skipped [rel: string] {
  let parts = ($rel | path split)
  ($parts | any {|p| $p in $SKIP_DIRS })
}

def lang-exts [lang: string] {
  match $lang {
    "python" => ["py"]
    "c" => ["c" "h"]
    "cpp" => ["c" "h" "cc" "cpp" "cxx" "hpp"]
    "go" => ["go"]
    "js" => ["js" "jsx" "mjs" "cjs"]
    "ts" => ["ts" "tsx" "mts" "cts"]
    _ => []
  }
}

def wants-file [lang: string, rel: string, root: string] {
  let ext = ($rel | path parse | get extension)
  if $ext not-in (lang-exts $lang) { return false }
  match $lang {
    "go" => ($rel | str ends-with "_test.go")
    "c" | "cpp" => (($rel | path split | drop 1 | any {|p| $p == "tests" }) or (($root | path basename) == "tests"))
    _ => true
  }
}

# Test names found on one line, as a list of strings.
def line-tests [lang: string, text: string] {
  match $lang {
    "python" => ($text | parse --regex '^\s*(?:async\s+)?def\s+(?P<n>test_\w*)\s*\(' | get n)
    "go" => ($text | parse --regex '^func\s+(?P<n>Test\w*)\s*\(' | get n)
    "c" | "cpp" => {
      let macro = ($text | parse --regex '^\s*TEST(?:_F)?\s*\(\s*(?P<s>\w+)\s*,\s*(?P<t>\w+)\s*\)' | each {|r| $"($r.s).($r.t)" })
      let func = ($text | parse --regex '^\s*(?:static\s+)?(?:void|int|bool)\s+(?P<n>test_\w*)\s*\(' | get n)
      $macro ++ $func
    }
    _ => ($text | parse --regex '\b(?:test|it)\s*\(\s*(?P<q>[''"`])(?P<n>.*?)(?P=q)' | get n)
  }
}

def scan-tests [from: string, lang: string] {
  let root = ($from | path expand)
  let files = (glob ($root | path join "**" "*") --no-dir | each {|f| $f | path relative-to $root } | where {|r| (not (skipped $r)) and (wants-file $lang $r $root) } | sort)
  mut rows = []
  for rel in $files {
    let body = (try { open --raw ($root | path join $rel) | decode utf-8 } catch { "" })
    let found = ($body | lines | enumerate | each {|l| line-tests $lang $l.item | each {|n| {file: ($rel | str replace --all "\\" "/"), line: ($l.index + 1), name: $n} } } | flatten)
    $rows = ($rows ++ $found)
  }
  # Repeated names in one file get a suffix so each id stays unique.
  mut seen = {}
  mut out = []
  for r in $rows {
    let key = $"($r.file)::($r.name)"
    let n = (($seen | get -o $key | default 0) + 1)
    $seen = ($seen | upsert $key $n)
    let id = (if $n == 1 { $key } else { $"($key)#($n)" })
    $out = ($out | append {id: $id, file: $r.file, line: $r.line, name: $r.name})
  }
  $out | sort-by file line id
}

def skeleton [lang: string, from: string, tests: list] {
  let head = [
    "# Port map. One [[test]] per original test; set `rust` to the name `cargo test -- --list` prints."
    "[port]"
    $"lang = \"($lang)\""
    $"from = \"($from)\""
    ""
  ]
  let body = ($tests | each {|t| ["[[test]]" $"original = \"($t.id)\"" "rust = \"\"" ""] } | flatten)
  ($head ++ $body | str join "\n")
}

def "main inventory" [--from: string = "", --lang: string = "", --write: string = ""] {
  let command = (argv (["nu" "rust-tops-port.nu" "inventory"] ++ (if $from != "" { ["--from" $from] } else { [] }) ++ (if $lang != "" { ["--lang" $lang] } else { [] }) ++ (if $write != "" { ["--write" $write] } else { [] })))
  if $from == "" or $lang == "" { return (fail $command "inventory needs --from <dir> and --lang <c|cpp|python|go|js|ts>") }
  if $lang not-in $LANGS { return (fail $command $"unknown --lang ($lang); use one of ($LANGS | str join ', ')") }
  if (not ($from | path exists)) or (($from | path type) != "dir") { return (fail $command $"--from is not a directory: ($from)") }
  let nu_err = (check-nu $from $command)
  if $nu_err != null { return $nu_err }
  let tests = (scan-tests $from $lang)
  mut written = ""
  if $write != "" {
    if ($write | path exists) { return (fail $command $"refusing to overwrite ($write)") }
    skeleton $lang $from $tests | save $write
    $written = $write
  }
  {
    ok: true
    command: $command
    data: {
      lang: $lang
      count: ($tests | length)
      tests: $tests
      written: $written
      next: "Fill in `rust` for each test, then run: nu rust-tops-port.nu verify --map port-map.toml"
    }
  } | to json
}

def finding [rules: any, id: string, file: string, line: int, message: string] {
  let r = ($rules | where id == $id | first)
  {rule: $id, kind: $r.kind, file: $file, line: $line, col: 1, message: $message, fix: $r.fix}
}

def listed-tests [path: string] {
  let r = (do { cd $path; ^cargo test -- --list | complete })
  if $r.exit_code != 0 { return {ok: false, error: ($"cargo test -- --list failed: ($r.stderr)" | str trim | str substring 0..<400)} }
  {ok: true, names: ($r.stdout | lines | parse --regex '^(?P<n>.+): test$' | get n | sort | uniq)}
}

def "main verify" [--map: string = "", --path: string = "."] {
  let command = (argv (["nu" "rust-tops-port.nu" "verify"] ++ (if $map != "" { ["--map" $map] } else { [] }) ++ ["--path" $path]))
  if $map == "" { return (fail $command "verify needs --map <port-map.toml>") }
  if not ($map | path exists) { return (fail $command $"map not found: ($map)") }
  let nu_err = (check-nu $path $command)
  if $nu_err != null { return $nu_err }
  let rules = (catalog)
  if $rules == null { return (fail $command "rules.toml not found beside the style skill") }
  let parsed = (try { open --raw $map | from toml } catch { null })
  if $parsed == null { return (fail $command $"map is not valid TOML: ($map)") }
  let entries = ($parsed | get -o test | default [])
  let map_rel = ($map | path basename)
  let map_lines = (open --raw $map | lines | enumerate | where {|l| $l.item | str starts-with "[[test]]" } | get index | each {|i| $i + 1 })
  mut findings = []
  # 1. Originals that no map entry mentions, when the map records where they live.
  let lang = ($parsed | get -o port.lang | default "")
  let from = ($parsed | get -o port.from | default "")
  if $lang != "" and $from != "" {
    let base = ($map | path expand | path dirname)
    let dir = (if ($from | path type) == "dir" { $from } else { $base | path join $from })
    if not ($dir | path exists) { return (fail $command $"map `from` is not a directory: ($from)") }
    let originals = (scan-tests $dir $lang | get id)
    let mapped = ($entries | get -o original | default [])
    for id in $originals {
      if $id not-in $mapped {
        $findings = ($findings | append (finding $rules "PORT-4" $map_rel 1 $"original test has no map entry: ($id)"))
      }
    }
  }
  # 2. Entries with no Rust name.
  for e in ($entries | enumerate) {
    let line = ($map_lines | get -o $e.index | default 1)
    let rust = ($e.item | get -o rust | default "")
    if ($rust | str trim | is-empty) {
      $findings = ($findings | append (finding $rules "PORT-4" $map_rel $line $"original test is unmapped: ($e.item | get -o original | default '')"))
    }
  }
  # 3. Rust names that cargo does not list.
  let listed = (listed-tests $path)
  if not $listed.ok { return (fail $command $listed.error) }
  for e in ($entries | enumerate) {
    let rust = ($e.item | get -o rust | default "" | str trim)
    if $rust != "" and $rust not-in $listed.names {
      let line = ($map_lines | get -o $e.index | default 1)
      $findings = ($findings | append (finding $rules "PORT-4" $map_rel $line $"mapped Rust test is not in cargo test -- --list: ($rust)"))
    }
  }
  # 4. The suite itself.
  let run = (do { cd $path; ^cargo test | complete })
  if $run.exit_code != 0 {
    $findings = ($findings | append (finding $rules "PORT-4" $map_rel 1 $"cargo test failed with exit code ($run.exit_code)"))
  }
  let sorted = ($findings | sort-by file line rule message)
  {
    ok: ($sorted | is-empty)
    command: $command
    data: {
      mapped: ($entries | length)
      rust_tests: ($listed.names | length)
      findings: $sorted
      judgment: ($rules | where {|r| ($r.kind == "judgment") and ("port" in $r.skills) } | each {|r| {id: $r.id, rule: $r.rule} } | sort-by id)
    }
  } | to json
}

def main [] {
  fail "nu rust-tops-port.nu" "pass inventory or verify"
}
