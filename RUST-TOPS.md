# Rust-TOPS adoption record

This repository is the source of the Rust-TOPS protocol and of `cargo-tops`,
and it adopts the protocol for its own code
([Hedronite/rust-tops](https://github.com/Hedronite/rust-tops), PROTOCOL.md §14.4).

| Item | Value |
| --- | --- |
| Protocol version | `1.4.0` |
| Spec | [`PROTOCOL.md`](PROTOCOL.md) in this tree |
| cargo-tops | built from this checkout (`cargo install --path crates/cargo-tops --locked`), package 0.4.0 |

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
```

CI runs the same gate in the `tops-gate` job.

## CRAP baseline

Baseline: [`crap-baseline.json`](crap-baseline.json) (`gates.crap.baseline`),
tracked. It started as the CRAP report of `main` at the 0.3.0 release merge.

To refresh it, run the gate, then `cargo tops crap-baseline`, and commit the
result with the change that moved the scores. Refresh it to record code that
got better or new code at or under the threshold. Never refresh it to let a
score rise.

## Exemptions in force

| Path | Symbol | Layer | Reason | Owner | Expires |
| --- | --- | --- | --- | --- | --- |

None.

## Known deviations

- The agent threshold is 6, tighter than the `binary-cli` class default of 8.
- `src/parse_oracle.rs` is a `#[cfg(test)]` module. `cargo llvm-cov` and
  `cargo crap` cannot tell, so it is judged as production.
