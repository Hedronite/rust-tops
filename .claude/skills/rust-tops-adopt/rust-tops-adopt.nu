#!/usr/bin/env nu
# Wraps `cargo tops init --skills` and `cargo tops check`. Does not pick a class.

def have [bin: string] {
  not ((which $bin | where type == external) | is-empty)
}

def quote-arg [a: string] {
  if ($a | str contains " ") { $"\"($a)\"" } else { $a }
}

def argv-line [args: list<string>] {
  $args | each {|a| quote-arg $a } | str join " "
}

def fail [command: string, error: string, exit_code: int] {
  {
    ok: false
    command: $command
    error: ($error | str trim | str substring 0..<400)
    exit: $exit_code
  }
}

def run-argv [args: list<string>] {
  let command = (argv-line (["cargo"] ++ $args))
  if not (have cargo) {
    return (fail $command "cargo: command not found" 127)
  }
  let r = (^cargo ...$args | complete)
  let text = ([$r.stderr $r.stdout] | str join "\n" | str trim)
  if $r.exit_code == 0 {
    {
      ok: true
      command: $command
      exit: 0
      stdout: ($r.stdout | str trim)
      stderr: ($r.stderr | str trim)
    }
  } else {
    fail $command $text $r.exit_code
  }
}

def "main adopt" [
  --class: string = ""
  --path: string = "."
  --name: string = ""
  --force
] {
  if ($class | str trim | is-empty) {
    return (fail "cargo tops init --skills" "pass --class (library-core, parser-codec, no-std-embedded, unsafe-kernel, binary-cli)" 2 | to json)
  }
  mut init_args = ["tops" "init" "--class" $class "--path" $path "--skills"]
  if not ($name | str trim | is-empty) {
    $init_args = ($init_args ++ ["--name" $name])
  }
  if $force {
    $init_args = ($init_args ++ ["--force"])
  }
  let init = (run-argv $init_args)
  if not $init.ok {
    return ({
      ok: false
      command: $init.command
      error: $init.error
      exit: $init.exit
      data: {init: $init}
    } | to json)
  }
  let check = (run-argv ["tops" "check" "--path" $path])
  let command = $"($init.command) && ($check.command)"
  if not $check.ok {
    return ({
      ok: false
      command: $command
      error: $check.error
      exit: $check.exit
      data: {init: $init, check: $check}
    } | to json)
  }
  {
    ok: true
    command: $command
    data: {init: $init, check: $check}
  } | to json
}

def main [] {
  fail "cargo tops init --skills" "pass adopt --class <class>" 2 | to json
}
