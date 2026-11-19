<!-- Rust-TOPS adoption record (PROTOCOL.md §14.4). `cargo tops init` fills
the marks in double braces from the rust-tops.yaml it writes. Keep this file
current by hand: one row per rust-tops.yaml, every exemption in force, every
deviation. -->
# Rust-TOPS adoption record

This repository adopts the Rust-TOPS protocol from
[Hedronite/rust-tops](https://github.com/Hedronite/rust-tops).

| Item | Value |
| --- | --- |
| Protocol version | `1.3.0` |
| Spec | <https://github.com/Hedronite/rust-tops/blob/v0.4.0/PROTOCOL.md> |
| cargo-tops | 0.4.0 (`cargo install cargo-tops --version 0.4.0 --locked`) |

## Crates and thresholds

One row per `rust-tops.yaml`. The numbers are that file's `gates`.

| Crate | Class | Instance | Line % | Region % | Per-file line % | CRAP agent | CRAP human |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `golden-crate` | `unsafe-kernel` | `rust-tops.yaml` | 95 | 90 | 90 | 6 | 4 |

## Run the gate locally

```sh
cargo tops check
cargo tops gate --max-tier 1
```

The gate fails when coverage is under a floor above, when a new production
function's CRAP is over the agent threshold, or when an existing function's
CRAP rises (PROTOCOL.md §2.1, §7.5, §16).

## CRAP baseline

Baseline: `crap-baseline.json` in the Cargo target directory (the default; not tracked)

The crap layer compares each production function with this file. Without it,
every function is judged as new. To refresh it, run the gate, then
`cargo tops crap-baseline`, and commit the result with the change that moved
the scores. Refresh it to record code that got better or code that is new
and under the threshold. Never refresh it to let a score rise.

## Exemptions in force

Copy each `exemptions` entry of each `rust-tops.yaml` here.

| Path | Symbol | Layer | Reason | Owner | Expires |
| --- | --- | --- | --- | --- | --- |

None.

## Known deviations

None recorded.
