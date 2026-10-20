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

- `cargo-tops` reads `rust-tops.yaml` with serde-saphyr 2.0.0. Free-form fields are `serde_json::Value`. `unsafe-libyaml` is no longer in the lock, and the `RUSTSEC-2024-0449` ignore is gone.
- Workspace `rust-version`, `rust-toolchain.toml`, and the CI toolchain pins are Rust 1.99.1, the current stable. The `msrv` job still runs `cargo check --workspace --all-features --locked` on that exact compiler.

## [0.1.4] — 2026-10-19

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Hard Bend gate (TOFU-pinned Bend 2.0.36), Lean 4.34.0 `--verdict` tier, and a law ownership guard. GitHub Actions in `.github/workflows/rust-tops.yml` are pinned by commit SHA. Protocol 1.0.0 is unchanged.

### Added

- `laws/bend.lock` pins Bend 2.0.36 (linux-x64, linux-arm64, darwin-arm64, darwin-x64) and `lean-4.34.0-linux.tar.zst`. The Bend upstream release is mutable and unsigned; the pin is trust-on-first-use.
- `scripts/fetch-bend.sh` verifies sha256 on every run and refuses an unverified platform. `scripts/bend-proof-ci.sh` refuses `BEND_GATE_NAMES_ONLY` and `RUST_TOPS_LAWS`. A missing `bend` exits 1.
- Jobs `bend-proof`, `bend-verdict` (cold wall time 21.87s, so it is a required check under the 5 minute line), `bend-gate-matrix`, and informational `bend-proof-macos` (never a required check).
- `law-diff` in its own workflow, and `.github/CODEOWNERS` for `@VirtualMachinist`.
- Template drift check: `laws/LAWS.bend` must match `crates/cargo-tops/templates/LAWS.bend`.

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
