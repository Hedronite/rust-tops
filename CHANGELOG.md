# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versioning follows [SemVer 2.0.0](https://semver.org/) **per artifact**:

| Plane | What | Where |
| --- | --- | --- |
| Protocol | rust-tops contract | `RUST_TOPS.md` header (`1.0.0`) and `schema/` |
| Package | `cargo-tops` and git tag | `v0.1.4`, Cargo `0.1.4` |

Do not mix planes. A protocol 1.0.0 body can ship under git tag `v0.1.4`.

## [Unreleased]

### Changed

- `cargo-tops` reads `rust-tops.yaml` with serde-saphyr 2.0.0 and `strict_booleans`. Unquoted `yes`, `no`, `on`, `off`, `y`, and `n` stay strings. A typed bool rejects `yes`. Free-form fields are `serde_json::Value`. `unsafe-libyaml` is no longer in the lock, and the `RUSTSEC-2024-0449` ignore is gone.
- Against serde_yaml 0.9: a leading-zero `017` reads as a number, integers outside the 64-bit range lose precision, custom tags are dropped, `~` in a string field is an error, and duplicate keys are an error.
- Workspace `rust-version`, `rust-toolchain.toml`, and the CI toolchain pins are Rust 1.99.1, the current stable. The `msrv` job still runs `cargo check --workspace --all-features --locked` on that exact compiler.
- `RUST_TOPS_RECORD_GOLDEN` rewrites the golden files for a local re-record. CI does not set it.

## [0.1.4] — 2026-10-19

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Hard Bend gate (TOFU-pinned Bend 2.0.36), Lean 4.34.0 `--verdict` tier, and a law ownership guard. GitHub Actions in `.github/workflows/rust-tops.yml` are pinned by commit SHA. Protocol 1.0.0 is unchanged.

### Added

- `laws/bend.lock` pins Bend 2.0.36 (linux-x64, linux-arm64, darwin-arm64, darwin-x64) and `lean-4.34.0-linux.tar.zst`. The Bend upstream release is mutable and unsigned; the pin is trust-on-first-use.
- `scripts/fetch-bend.sh` verifies sha256 on every run and refuses an unverified platform. `scripts/bend-proof-ci.sh` refuses `BEND_GATE_NAMES_ONLY` and `RUST_TOPS_LAWS`. A missing `bend` exits 1.
- Jobs `bend-proof`, `bend-verdict` (cold wall time 21.87s, so it is a required check under the 5 minute line), `bend-gate-matrix`, and informational `bend-proof-macos` (never a required check).
- `law-diff` in its own workflow, and `.github/CODEOWNERS` for `@VirtualMachinist`.
- Template drift check: `laws/LAWS.bend` must match `crates/cargo-tops/templates/LAWS.bend`.

### Changed

- `laws/PROOF.bend` is laid out one claim per line and one call per line, with a `#` comment above each group of laws (H-01..H-10, H-11..H-20, H-21..H-30, H-31..H-40, H-41..H-53). `scripts/gen-laws.rs` writes the layout and stays the only writer. The 53 claim strings are unchanged and in the same order; the call tuple is still right-nested. `laws/LAWS.bend` and `laws/NAMES` are byte-identical. Bend 2.0.36 passes `bend PROOF.bend` and `--verdict`.
- `scripts/law-diff.rs` reads `main`'s signature from `def main(` to the header colon across lines, drops `#` comment lines, and compares the same claim strings. A layout change is no change; an edited claim still needs `laws-change-approved`. `scripts/testdata/PROOF.one-line.bend` keeps the old one-line proof as the fixture for that test.
- `scripts/bend-verdict-step.sh` reads `PIPESTATUS` once. A `tee` failure now reports `bend-verdict: tee exited N` and writes no `VERDICT-TIMING.md`. The old second read was the assignment's status and was always 0.
- `scripts/test-bend-gate.sh` exits 1 when `CI` is set and any case was skipped. A local run keeps its skips and exits 0.
- `scripts/bend-gate.sh` and `scripts/fetch-bend.sh` refuse `RUST_TOPS_ROOT` when `CI` is set. Nix still sets it without `CI`.
- `cargo-deny` is pinned to `0.18.9` in `.github/workflows/rust-tops.yml`.

Protocol 1.0.0 is unchanged.

### Note

`config/drop-in/rust-tops.yml` and `crates/cargo-tops/templates/drop-in/rust-tops.yml` stay tag-pinned. Repinning them changes `cargo tops init` and is a follow-up. Nix `bend-proof` is local feedback: no CI job runs `nix flake check`. Darwin's `checks.bend-proof` is absent.

## [0.1.3] — 2026-09-25

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Maintainer tooling: `scripts/gen-laws.py` → `scripts/gen-laws.rs` (native Cargo script, RFC 3502, std-only). Regenerated H-01..H-53 pack is byte-identical. Package-plane wiring bumped to 0.1.3.

### Changed

- `scripts/gen-laws.rs` replaces the Python generator; regenerate with `cargo +nightly -Zscript scripts/gen-laws.rs`.
- Pins table and package metadata updated to 0.1.3.

### Note

Vendored `crates/cargo-tops/templates/LAWS.bend` is a byte-copy of `laws/LAWS.bend` today; a drift guard is queued for a later slice.

## [0.1.2] — 2026-09-25

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Provisional Bend2 pack (`laws/LAWS.bend`, 53 harvest laws H-01..H-53) plus a Nix flake overlay (`cargo-tops`, `rust-tops-laws`). Not a protocol bump. Schema stays Bend-free. Soft rows map to `Ack`; hard rows map to `Refuse`.

### Added

- [`laws/`](laws/) Bend pack + [`PROOF.bend`](laws/PROOF.bend). Gate: `./scripts/bend-gate.sh`.
- Nix flake overlay. Adopting flakes take `overlays.default`.
- `cargo tops init` writes `LAWS.bend`.

## [0.1.1] — 2026-09-25

Compatible 0.x patch. `cargo-tops` is now a crates.io crate (`cargo install cargo-tops`). Drop-in kit is vendored under `crates/cargo-tops/templates/` so `cargo package` is self-contained. CLI verbs `init` / `check` / `gate` unchanged. Protocol plane stays 1.0.0.

### Fixed

- `include_str!` paths pointed at workspace files outside the crate, so `cargo publish` could not verify the tarball.

## [0.1.0] — 2026-09-25

First public kit. Protocol body is rust-tops **1.0.0**. Package / git tag is **0.1.0** (first published tag; not `0.0.x`).

### Added

- Normative protocol [`RUST_TOPS.md`](RUST_TOPS.md) + JSON Schema.
- Class profiles, example instance, tool-lock template.
- Drop-in kit: `clippy.toml`, nextest, mutants, crap, deny, GHA floor.
- Harvested GHA floor: sysdeps-before-all-features, honest MSRV, feature-matrix, nonempty record-only coverage artifacts. No job-level `RUSTFLAGS`. No `--fail-under-lines`.
- [`POLICIES.md`](POLICIES.md) gates/checks from applying the protocol.
- [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) candidate laws (not protocol 1.0).
- `cargo-tops` CLI: `init` / `check` / `gate` (orchestrates existing Cargo tools).
- Fixtures: `examples/parser-codec`, `examples/no-std-embedded`.

[0.1.4]: https://github.com/Hedronite/rust-tops/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/Hedronite/rust-tops/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/Hedronite/rust-tops/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Hedronite/rust-tops/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Hedronite/rust-tops/releases/tag/v0.1.0
