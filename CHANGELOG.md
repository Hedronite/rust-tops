# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versioning follows [SemVer 2.0.0](https://semver.org/) **per artifact**:

| Plane | What | Where |
| --- | --- | --- |
| Protocol | rust-tops contract | `RUST_TOPS.md` header (`1.0.0`) and `schema/` |
| Package | `cargo-tops` and git tag | `v0.1.2`, Cargo `0.1.2` |

Do not mix planes. A protocol 1.0.0 body can ship under git tag `v0.1.2`.

## [Unreleased]

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

[Unreleased]: https://github.com/Hedronite/rust-tops/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/Hedronite/rust-tops/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Hedronite/rust-tops/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Hedronite/rust-tops/releases/tag/v0.1.0
