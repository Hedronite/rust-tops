# Agent standing orders (Rust-TOPS 1.0)

Goal: correct, dense, fast Rust.
Production: fewest necessary lines that preserve named invariants and measured performance.
Tests: as many as the contract needs. Never write tests by reading the implementation.

## Do this

1. Read the module rustdoc / `rust-tops.yaml` contract first.
2. Write or update the oracle (table, property, acceptance) from that contract.
3. Implement in the smallest production shape that satisfies it.
4. Run the PR loop and stop when it is green.

```text
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features --profile ci
cargo llvm-cov nextest --workspace --all-features --lcov --output-path target/coverage/lcov.info
test -s target/coverage/lcov.info
cargo crap --lcov target/coverage/lcov.info
cargo mutants --in-diff <diff>
```

Do not set `RUSTFLAGS=-D warnings` in the environment. Do not pass
`--fail-under-lines` unless the instance `rust-tops.yaml` class floor says so
and the LCOV is already nonempty.

## Gates

- New production functions: CRAP ≤ 6 (humans target ≤ 4).
- Coverage: class minimum in `rust-tops.yaml` (parsers: 95% line / 90% region).
- Mutation on the diff: 0 unexplained survivors. Never `--baseline skip`.
- Every claimed invariant has a `proptest` (or Kani proof).
- Every untrusted byte boundary has a fuzz target.
- Every `unsafe` block has a safety comment and a Miri test.
- Hot paths listed in `rust-tops.yaml` do not pick up an unexplained Callgrind Ir regression.
- Declared MSRV equals the locked graph's max rustc, including target-gated deps.
- FFI / C-ABI features: install sysdeps in every job that builds `--all-features`.
- Record-only jobs still fail on empty artifacts (`test -s` is its own step).

## Forbidden

- Generating tests from the current function body.
- Splitting a function only to lower CRAP (`scar_guard`). No `*_part_2`.
- Inventing a parallel spec type when the interned domain type already exists.
- `#[mutants::skip]` without an equivalent-mutant reason.
- New trait / builder / middleware with a single impl.
- `.clone()` or `format!` on a listed hot path without a why-comment.
- `unwrap` / `expect` on library success paths except programmer-invariant bugs.
- Enabling `clippy::pedantic` or `clippy::restriction` as a group.
- Adding a dependency that does not delete production lines or buy measured safety/perf.
- Masking clippy with stacked `allow` / job-level `RUSTFLAGS` so a real lint never fires.
- Dropping a class-required CI job to look greener on a headerless runner.

## If a gate fails

- Coverage hole → add a test for the **specified** missing branch, not a tautology.
- High CRAP → reduce real branching or cover the risky arm. Do not extract `foo_part_2`.
- Surviving mutant → assert the behavior the mutant broke.
- Perf miss → profile, then algorithm / alloc / layout. `unsafe` is last and Miri-clean.
- Empty LCOV / CRAP JSON → the recorder never ran; fix the kit, do not skip.
- Spawned-bin ENOENT under llvm-cov → honor `CARGO_BIN_EXE_*` / `$CARGO_TARGET_DIR` / one-level `target/` scan.

See [`POLICIES.md`](POLICIES.md) for the CI floor and [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) for candidate fail-closed laws.
