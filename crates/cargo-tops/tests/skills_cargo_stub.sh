#!/bin/sh
# Test double for `cargo`. Modes are selected by CARGO_STUB_MODE.
mode=${CARGO_STUB_MODE:-pass}
target=${CARGO_TARGET_DIR:?}
report="$target/rust-tops/cases.json"
write_cases() {
  mkdir -p "$target/rust-tops"
  printf '%s\n' "$1" > "$report"
}
case "$mode" in
  pass)
    write_cases '{"passed":true,"laws":[{"law":"fmt_is_tier0","status":"checked","verdict":"Allow"},{"law":"lock_honesty","status":"checked","verdict":"Ack"}]}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  not-evaluated)
    write_cases '{"passed":true,"laws":[{"law":"evidence_not_self_served","status":"not-evaluated","verdict":null,"reason":"no fact","reason_code":"no_fact_reader"}]}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  refuse)
    write_cases '{"passed":true,"laws":[{"law":"lock_honesty","status":"checked","verdict":"Refuse"}]}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  missing)
    printf 'fail: 1 problem(s), 0 command(s) run\n'
    exit 1
    ;;
  no-tops)
    printf '%s\n' "error: no such command: \`tops\`" >&2
    exit 1
    ;;
  run-dependent)
    write_cases '{"passed":true,"laws":[{"law":"perf_without_runner_is_not_gate","status":"not-evaluated","verdict":null,"reason":"no performance layer is enabled at or below this tier","reason_code":"run_dependent"}]}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  bad-json)
    write_cases 'not-json'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  coverage)
    write_cases '{"passed":true,"laws":[{"law":"fmt_is_tier0","status":"checked","verdict":"Allow"}]}'
    printf '%s\n' '==> [static-lint] cargo fmt --all -- --check'
    printf '%s\n' '==> [coverage] cargo llvm-cov nextest'
    printf '%s\n' ''
    printf '%s\n' 'fail: 1 problem(s), 2 command(s) run'
    printf '%s\n' '  - coverage: `cargo llvm-cov nextest` failed'
    exit 1
    ;;
  *)
    printf 'unknown mode %s\n' "$mode" >&2
    exit 2
    ;;
esac
