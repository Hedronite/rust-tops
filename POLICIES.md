# Rust-TOPS policies (gates / checks)

This is the **practice layer** harvested from applying protocol 1.0.0.
It does not amend [`RUST_TOPS.md`](RUST_TOPS.md). Instance tightness lives in
`rust-tops.yaml`. CI floor lives in [`config/drop-in/rust-tops.yml`](config/drop-in/rust-tops.yml).

## Floor, not ceiling

The drop-in workflow is the jobs every adopting crate must be able to run.
Class overlays (Darwin, mutants-on-diff, fuzz smoke, Kani, Callgrind) may be
**added**. They may not be dropped to look greener on a headerless runner.

Job ids are load-bearing once branch protection pins them. Renaming a job or
adding a matrix key is a product change — it forks the required-check set.

## CI floor (must)

| Gate | Rule |
| --- | --- |
| `fmt-clippy-deny` | `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`, `cargo deny check` |
| `nextest` | `cargo nextest --profile ci` + `cargo test --doc`, both `--all-features` |
| `msrv` | `cargo check --workspace --all-features --locked` on the declared MSRV |
| `feature-matrix` | every shipped feature, on the OS that actually builds it |
| `coverage-crap` | record LCOV + CRAP JSON; both artifacts nonempty |

Do **not**:

- set job-level `RUSTFLAGS=-D warnings` (clippy already denies; a second site double-denies and masks real lints)
- put `--fail-under-lines` in the drop-in (floors live in `rust-tops.yaml` by class)
- fold `test -s` into the previous command's argv (YAML orphans become cargo flags)
- skip sysdeps because Ubuntu happens to compile without the FFI feature on

## Sysdeps and MSRV

- A feature that links a C ABI is a **build** requirement. `--all-features` counts.
- Install headers / libs in **every** job that enables that feature.
- Declared MSRV = max rustc demanded by the locked graph, **including target-gated deps**. Two targets voting different rustc is not a vote; take the max or split the lock.

## Coverage kit

`cargo llvm-cov --target-dir …` relocates workspace binaries. Integration tests
that spawn a bin via `CARGO_BIN_EXE_*` or a hardcoded `target/debug/<bin>` will
ENOENT. Discover the bin from `CARGO_BIN_EXE_*`, then `$CARGO_TARGET_DIR`, then
a **one-level** scan of `target/*/debug/<bin>` — never hardcode `llvm-cov-target`.

Record-only is not a skip: empty LCOV / empty CRAP JSON is a fail.

## Clippy density

- Trait methods required by a foreign trait are clippy-exempt for `too_many_arguments`.
- Inherent helpers on the same type are **not**. Intern an existing type; do not invent a parallel spec type when the interned node already exists.
- `cognitive-complexity` 12, `too-many-lines` 40, `too-many-arguments` 6 (see `config/drop-in/clippy.toml`).
- Never enable `clippy::pedantic` or `clippy::restriction` as a group.

## scar_guard

High CRAP → reduce real branching or cover the risky arm.
Never extract `foo_part_2`. Never split a function only to please CRAP.
A density extract must have a **domain name**.

## Tests

- Oracle before impl. Never write tests by reading the function body.
- Every claimed invariant has a `proptest` (or a Kani proof).
- Every untrusted byte boundary has a fuzz target (parser-codec class).
- `#[mutants::skip]` needs an equivalent-mutant reason.
- `--baseline skip` is a lie; baseline must be green before mutants.

## Overlays (not the floor)

| Overlay | When |
| --- | --- |
| `mutants-diff` | class overlay; diff-scoped; required only after one measured GHA wall time |
| Darwin / other `cfg(target_os)` | every cfg needs a lane; file the gap, do not drop the cfg |
| fuzz-smoke | parser-codec; scheduled/manual until a known crash is closed. Crash = fail; timeout = infra |
| Callgrind / Criterion | no runner → not a gate |

## Agent standing orders

See [`AGENTS.md`](AGENTS.md). Coding agents fail closed on the gates above.
Bend2 encode of these policies: [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) (candidates, not protocol 1.0).
