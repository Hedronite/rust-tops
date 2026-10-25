# Agent standing orders (Rust-TOPS 1.1.0)

Goal: correct, dense, fast Rust.
Production: fewest necessary lines that preserve named invariants and measured performance.
Tests: as many as the contract needs. Never write tests by reading the implementation.

Protocol 1.1.0. The text is [`RUST_TOPS.md`](RUST_TOPS.md). This file is the harness.
The `laws` layer is optional. A 1.0.0 instance with no `laws` layer still validates, and `gate` does not call Bend.

## Do this

1. Read the module rustdoc / `rust-tops.yaml` contract first.
2. Write or update the oracle (table, property, acceptance) from that contract.
3. Implement in the smallest production shape that satisfies it.
4. `cargo tops check`, then `cargo tops gate`. Stop when the gate is green.

`cargo tops init --class <class>` adopts the kit. It writes `rust-toolchain.toml` with `channel` set from `crate.msrv`, plus `LAWS.bend`, `laws/bend.lock`, and `scripts/fetch-bend.sh`. Those three are pinned. Do not edit them to get a green gate. `cargo tops init --skills` also writes the agent skills, a one-line `CLAUDE.md` that imports this file, and [`docs/REVIEW.md`](docs/REVIEW.md).

`cargo tops gate` runs the mapped layers, then `laws` when that layer is enabled. A Bend `Refuse` fails the run. `not-evaluated` is never a pass. `bend` stops at the first refusal; later laws are `unreported`, which is not a pass. Read `rust-tops/cases.json` under the Cargo target directory (`$CARGO_TARGET_DIR` when set, otherwise `cargo metadata`'s `target_directory`). Ignore a file from an earlier run.

The commands the gate runs, when those layers are enabled:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
cargo nextest run --workspace --all-features --profile ci
cargo test --doc --workspace --all-features
cargo llvm-cov nextest --workspace --all-features --lcov --output-path target/coverage/lcov.info
test -s target/coverage/lcov.info
cargo crap --lcov target/coverage/lcov.info
cargo mutants --in-diff <diff>
```

Do not set `RUSTFLAGS=-D warnings`. Do not pass `--fail-under-lines` unless the instance class floor says so and the LCOV is already nonempty. `cargo tops gate` does not pass it.

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

## Module structure

- Put a new responsibility in a module named for that responsibility. Do not append it to the file that is already open.
- One `mod` per file, in one layout, so a path finds the item.
- `pub use` only for a deliberate facade (`lib.rs`, `prelude`) or a dependency type in the public API. After a move, fix the call sites. Do not re-export to silence the compiler.
- `#[path]` only for a `cfg`-selected platform module. `include!` only for generated code under `OUT_DIR`. Otherwise a normal `mod`. `include_str!` and `include_bytes!` are for data; keep those files small.
- Explicit imports in production code. `use super::*` inside `#[cfg(test)]` is fine.
- When a file or function no longer fits one sentence, split it on a named seam. Never split only to quiet a metric.

One restriction or pedantic lint may be enabled by name. `clippy::pedantic` and `clippy::restriction` as a group stay off. A threshold in `clippy.toml` does nothing until that lint is enabled.

## Review

The reviewer is a fresh agent that did not write the diff. Write the inputs before reading the implementation, run them, and review every fix commit again. High-risk changes (business rules, thresholds, scoring, pricing, or a validation gate) get a second reviewer on a different model. The full gate is [`docs/REVIEW.md`](docs/REVIEW.md). This does not change CRAP, coverage, or mutation gates.

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
- Editing `LAWS.bend`, `laws/bend.lock`, or `scripts/fetch-bend.sh` so a law passes.
- Counting `not-evaluated` or `unreported` as a pass.

## If a gate fails

- Coverage hole → add a test for the specified missing branch, not a tautology.
- High CRAP → reduce real branching or cover the risky arm. Do not extract `foo_part_2`.
- Surviving mutant → assert the behavior the mutant broke.
- Perf miss → profile, then algorithm / alloc / layout. `unsafe` is last and Miri-clean.
- Empty LCOV / CRAP JSON → the recorder never ran; fix the kit, do not skip.
- Spawned-bin ENOENT under llvm-cov → honor CARGO_BIN_EXE_* / $CARGO_TARGET_DIR / one-level `target/` scan.
- Bend `Refuse` → read that law in rust-tops/cases.json and fix the fact it read. Do not edit the pinned kit files.
- `not-evaluated` → the law was not checked. It is not a pass. Read `reason` in `cases.json`. If the runner has no fact for that law, leave it. Do not edit the pinned kit files and do not encode it as `Allow`.
- Tool missing → install it. `cargo-tops` does not vendor engines. A missing tool is a fail, not a skip.

See [`POLICIES.md`](POLICIES.md) for the CI floor and [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) for the law catalog. A `Refuse` from an enabled `laws` layer fails the run ([`RUST_TOPS.md`](RUST_TOPS.md) §3.1, §22).
