# Rust-TOPS adoption record

This repository is the source of the Rust-TOPS protocol and of `cargo-tops`,
and it adopts the protocol for its own code
([Hedronite/rust-tops](https://github.com/Hedronite/rust-tops), PROTOCOL.md §14.4).

| Item | Value |
| --- | --- |
| Protocol version | `1.5.0` |
| Spec | [`PROTOCOL.md`](PROTOCOL.md) in this tree |
| cargo-tops | built from this checkout (`cargo install --path crates/cargo-tops --locked`), package 0.5.0 |
| Enforcement | `enforce: ratchet` (PROTOCOL.md §2.3) |

## Crates and thresholds

One row per `rust-tops.yaml`. The numbers are that file's `gates`.

| Crate | Class | Instance | Line % | Region % | Per-file line % | CRAP agent | CRAP human |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `cargo-tops` | `binary-cli` | [`rust-tops.yaml`](rust-tops.yaml) | 80 | 75 | 90 | 6 | 4 |

The instance sits at the workspace root and names `cargo-tops`, so the
floors and the CRAP rules judge `crates/cargo-tops/` only. `xtask` and the
two `examples/` fixtures are outside that crate.

## Run the gate locally

```sh
cargo install --path crates/cargo-tops --locked
cargo tops check
cargo tops gate --max-tier 1
cargo tops baseline --check --base origin/main
```

CI runs the same gate in the `tops-gate` job.

## CRAP baseline

Debt ledger: [`rust-tops-baseline.json`](rust-tops-baseline.json), Baseline v2
(PROTOCOL.md §16.1), tracked. It lists only the functions of `cargo-tops` that
are over the cap. Under `ratchet` a listed function may score up to its entry
plus 0.01 (a rise that small is noise), every other function is held to the cap,
and a missing ledger fails the gate.

The same ledger can hold coverage below a floor, in a `coverage` object
(PROTOCOL.md §7.5); a listed percent may fall by no more than 0.01. This crate
meets its floors, so its ledger has no `coverage` object.

After a passing gate, run `cargo tops baseline` and commit the result with the
change that moved the numbers. The refresh lowers or removes function entries,
raises or removes coverage entries, and never adds one.

In a pull request, run `cargo tops baseline --check --base origin/main` on a
branch that is up to date with `main`. No CI job runs this check yet. Debt that
has to grow is written by hand, with a `regressions` record (PROTOCOL.md §16.2),
and reviewed like any other change to the gate.

## Exemptions in force

| Path | Symbol | Layer | Reason | Owner | Expires |
| --- | --- | --- | --- | --- | --- |

None.

## Known deviations

- The agent threshold is 6, tighter than the `binary-cli` class default of 8.
- `src/parse_oracle.rs` is a `#[cfg(test)]` module. `cargo llvm-cov` and
  `cargo crap` cannot tell, so it is judged as production.
