# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versioning follows [SemVer 2.0.0](https://semver.org/) **per artifact**:

| Plane | What | Where |
| --- | --- | --- |
| Protocol | rust-tops contract | `RUST_TOPS.md` header (`1.0.0`) and `schema/` |
| Package | `cargo-tops` and git tag | `v0.1.0`, Cargo `0.1.0` |

Do not mix planes. A protocol 1.0.0 body can ship under git tag `v0.1.0`.

## [Unreleased]

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

[Unreleased]: https://github.com/Hedronite/rust-tops/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Hedronite/rust-tops/releases/tag/v0.1.0
