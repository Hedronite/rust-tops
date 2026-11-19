#!/bin/sh
# Test double for `cargo`. Modes are selected by CARGO_STUB_MODE.
# It writes the report a real `cargo tops gate` writes: report.json under the
# target directory, with the exit code, the layers and the laws.
mode=${CARGO_STUB_MODE:-pass}
target=${CARGO_TARGET_DIR:?}
report="$target/rust-tops/report.json"
LINT='{"id":"static-lint","tier":0,"verdict":"pass","argv":["cargo fmt --all -- --check"],"tool_version":"1.99.1","seconds":0.5}'
write_report() {
  mkdir -p "$target/rust-tops"
  printf '{"protocol_version":"1.3.0","crate":"stub","max_tier":1,"exit_code":%s,"ok":%s,"layers":[%s],"laws":[%s]}\n' "$1" "$2" "$3" "$4" > "$report"
}
case "$mode" in
  pass)
    write_report 0 true "$LINT" '{"law":"fmt_is_tier0","status":"checked","verdict":"Allow"},{"law":"lock_honesty","status":"checked","verdict":"Ack"}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  not-evaluated)
    write_report 0 true "$LINT" '{"law":"evidence_not_self_served","status":"not-evaluated","verdict":null,"reason":"no fact","reason_code":"no_fact_reader"}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  refuse)
    write_report 0 true "$LINT" '{"law":"lock_honesty","status":"checked","verdict":"Refuse"}'
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
    write_report 0 true "$LINT" '{"law":"perf_without_runner_is_not_gate","status":"not-evaluated","verdict":null,"reason":"no performance layer is enabled at or below this tier","reason_code":"run_dependent"}'
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  bad-json)
    mkdir -p "$target/rust-tops"
    printf 'not-json\n' > "$report"
    printf 'ok: 1 command(s) passed\n'
    exit 0
    ;;
  coverage)
    COV='{"id":"coverage","tier":1,"verdict":"fail","argv":["cargo llvm-cov nextest"],"tool_version":"0.9.1","seconds":2.25}'
    write_report 1 false "$LINT,$COV" '{"law":"fmt_is_tier0","status":"checked","verdict":"Allow"}'
    printf '%s\n' 'fail: 1 problem(s), 2 command(s) run'
    printf '%s\n' '  - coverage: `cargo llvm-cov nextest` failed'
    exit 1
    ;;
  tool-missing)
    MUT='{"id":"mutation","tier":1,"verdict":"tool-missing","argv":["cargo mutants"],"tool_version":"","seconds":0}'
    write_report 3 false "$MUT" ''
    printf '%s\n' 'error: cargo-mutants: command not found' >&2
    exit 3
    ;;
  config-invalid)
    write_report 2 false '' ''
    printf '%s\n' 'error: rust-tops.yaml: crate.name is required' >&2
    exit 2
    ;;
  *)
    printf 'unknown mode %s\n' "$mode" >&2
    exit 64
    ;;
esac
