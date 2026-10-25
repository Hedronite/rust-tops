# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versioning follows [SemVer 2.0.0](https://semver.org/) **per artifact**:

| Plane | What | Where |
| --- | --- | --- |
| Protocol | rust-tops contract | `RUST_TOPS.md` header (`1.1.0`) and `schema/` |
| Package | `cargo-tops` and git tag | `v0.2.0`, Cargo `0.2.0` |

Do not mix planes. Protocol 1.0.0 shipped under tags `v0.1.0` through `v0.1.4`. Protocol 1.1.0 first ships with package 0.2.0.

## [Unreleased]

### Added

- Three agent skills, `rust-tops-adopt`, `rust-tops-gate`, and `rust-tops-review`. Each is a `SKILL.md` plus a Nushell script that prints `{ok, command, data}`. The script wraps `cargo tops`; it does not re-implement it. `rust-tops-gate` returns `ok: true` only when `cargo tops gate` exits 0 and every law row is `checked` with verdict `Allow` or `Ack`. `data.gate_green` is true when that command exits 0 and every checked law passed. A `not-evaluated` law with `reason_code` `no_fact_reader` is a `data.gaps` item. One with `reason_code` `run_dependent` stays in `data.fixes`. Either list keeps `ok` false. The script does not read the report's top-level pass bit. A non-zero exit reports that code and the stderr tail before the report is read.
- `cargo tops init --skills` writes that layout as ordinary files: `.agents/skills/<name>/`, `.claude/skills/<name>/`, `.cursor/skills/<name>/`, and `.grok/skills/<name>/` hold the same bytes. It does not create symlinks. A drift test in `cargo test --workspace` fails when any copy differs or is a symlink. It also writes a one-line `CLAUDE.md` (`@AGENTS.md`) and `docs/REVIEW.md`. Default `cargo tops init` does not write those files.
- The repository `nextest` job installs Nushell 0.108.0 from the official GitHub release tarball `nu-0.108.0-x86_64-unknown-linux-gnu.tar.gz` (sha256 `f0daaa0256b0f7dd5f4d9925b4d5522d8de5e947ed9ad64a43526e5cffeb05df`) and sets `RUST_TOPS_REQUIRE_NU` so the skill envelope tests fail when `nu` is missing. No other job changes. The drop-in workflow is unchanged.
- [`docs/REVIEW.md`](docs/REVIEW.md), the review gate. It does not change CRAP, coverage, or mutation.

### Changed

- `cargo tops` and `cargo-tops` are the same binary. Cargo passes `tops` as the first argument; the binary drops that word before clap parses. `cargo tops check` and `cargo-tops check` both run.
- `rust-tops-gate` deletes the previous `cases.json` with Nushell's `rm`, so a PATH without coreutils still prints JSON. `rust-tops-review` `diff` includes `data.untracked`, the same name list as `files`. Untracked bodies stay out of the patch.
- A `not-evaluated` row in `cases.json` includes `reason_code`. `no_fact_reader` means this release has no reader for that law. `run_dependent` means the reader exists and this run did not give it a fact.
- `AGENTS.md` (repo root, the init template, and the init golden) is the protocol 1.1.0 harness: `cargo tops check` then `cargo tops gate`, the optional `laws` layer, Refuse / not-evaluated / unreported, and the pinned kit files. It adds the module-structure rules and a short review section.
- `cargo tops gate` writes `cases.bend` and `cases.json` under `$CARGO_TARGET_DIR/rust-tops` when that variable is set, otherwise under `cargo metadata`'s `target_directory`, otherwise under `<project>/target/rust-tops`.
- [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) states that the laws layer is normative in protocol 1.1.0. [`RUST_TOPS.md`](RUST_TOPS.md) §14.3 points at the harness `init` writes.

## [0.2.0] - 2026-10-21

**Breaking 0.x minor.** Protocol plane moves to **1.1.0** (additive `laws` layer). Package plane moves to **0.2.0**.

Pre-1.0 SemVer: the incompatible gate change below takes the minor from published `0.1.4` to `0.2.0`.

### Breaking

- With the `laws` layer enabled, and not on `--dry-run`, `cargo tops gate` resolves Bend before any other layer ([#10]). It needs the version in `laws/bend.lock` (2.0.36), looked up on `PATH` and then in `$HOME/.bend/bin`. If that binary is absent, `gate` runs `sh scripts/fetch-bend.sh`. A missing `laws/bend.lock`, or a lock with no `[bend] version`, exits 1 before any other layer runs. A missing `scripts/fetch-bend.sh` exits 1 before any other layer runs when that Bend is not already installed. A failed fetch exits 1 the same way. Those stops print `fail: laws: ...`. In 0.1.4, `gate` never looked for Bend. A `Refuse` from `bend` fails the run.
- A failing run that gets past that check prints `fail: N problem(s), M command(s) run`. 0.1.4 printed `fail: N of M command(s)`. The success line stays `ok: N command(s) passed`.
- `cargo tops init` writes `rust-toolchain.toml` with `channel` set from the profile's `crate.msrv`. Every bundled profile sets `crate.msrv` to `1.88`.
- The drop-in workflow does not install `stable`. Each job reads `channel` from `rust-toolchain.toml` and passes it to `dtolnay/rust-toolchain` at commit `683b588`, which has no default toolchain. A missing `rust-toolchain.toml` fails that read. An empty `channel` fails the install. The `msrv` job lost its unsubstituted `@{MSRV}` placeholder.

### Migration from 0.1.4

- A config with `protocol.version: "1.0.0"` and no `laws` layer still validates. The schema accepts any `MAJOR.MINOR.PATCH` version, and `laws` is an optional layer id. `cargo tops check` accepts that file. `cargo tops gate` does not call Bend. The failure summary above is the change for that repo.
- To adopt the layer, set `protocol.version: "1.1.0"` and add `{ id: laws, enabled: true, tier: 0, runner: bend }`. The schema requires `id`, `enabled`, and `runner` on every layer. The bundled template also sets `scope: workspace`.
- Copy in `laws/bend.lock` and `scripts/fetch-bend.sh` (mode 0755) beside `LAWS.bend`. `cargo tops init` writes those files and `rust-toolchain.toml`.
- `init` skips a file that already exists and prints `skip existing <path>`. The overwrite flag is `--force` (`cargo tops init --force`). `cargo-tops init --help` prints `--force` as "Overwrite existing protocol files". There is no short flag.

### Added

- `RUST_TOPS.md` §3.1 defines the `laws` layer. The runner chooses one case per law from the crate's files or this gate's reports. `bend` judges a program that imports `LAWS.bend`. A `Refuse` fails the run, an `Ack` is recorded, and a law with no fact is `not-evaluated`, never a pass. §15 lists `LAWS.bend`, `laws/bend.lock`, and `scripts/fetch-bend.sh`. §22 makes a `Refuse` from an enabled `laws` layer normative.
- `schema/rust-tops.schema.json` adds `laws` to the layer-id enum. Both schema `$id`s move to `v1.1.0`. `config/rust-tops.lock.example` sets `[notes] schema` to `1.1.0` (it was `1.0.0`). `init` does not write `rust-tops.lock`.
- `cargo tops init` writes `protocol.version: "1.1.0"` and the enabled `laws` layer. Beside `LAWS.bend` it copies `laws/bend.lock` and `scripts/fetch-bend.sh`. Both are byte copies, held by drift tests.
- `init` appends one job per job keyword §4 requires for the class: `fuzz-smoke` (parser-codec), `miri` (no-std-embedded, unsafe-kernel), `kani` and `callgrind-hot` (unsafe-kernel). Each runs the real tool and fails when it is missing: `cargo +nightly fuzz list` must name a target, `cargo +nightly miri test`, `cargo kani` (kani-verifier 0.68.0), `cargo bench --bench hot` under valgrind (gungraun-runner 0.20.0). library-core and binary-cli get no extra jobs. `config/drop-in/` mirrors the templates.
- A fresh `cargo tops init && cargo tops gate` exits 0 for every class `init` offers (tested with a stubbed Bend and with Bend 2.0.36).
- `cargo tops gate` runs the `laws` layer after every other layer, by the same tier rule. It writes `target/rust-tops/cases.bend`, runs the pinned `bend` on it, and writes `target/rust-tops/cases.json` (law, verdict function, case, files read, `bend`'s verdict, or the `not-evaluated` reason). It prints every law.
- Fifteen laws get a case: `ci_matrix_matches_class`, `fmt_is_tier0`, `lock_honesty`, `msrv_enforced`, `clippy_toolchain_pinned`, `no_pedantic_group`, `baseline_green_before_mutants`, `record_only_artifacts_are_nonempty`, `missing_tool_not_green`, `crap_wires_coverage`, `coverage_features_match_workspace`, `perf_without_runner_is_not_gate`, `artifact_check_is_its_own_step`, `drop_in_is_a_floor_not_a_ceiling`, and `single_deny_site`. Each can still be `not-evaluated` when its file or report is absent. The other 38 are always `not-evaluated`. `evidence_not_self_served` is one of them: no file shows intent.
- Nix: `overlays.default` exposes `rust-tops-bend` (Bend 2.0.36 from the lock sha256) on `x86_64-linux` and `aarch64-linux`, and `checks.bend-proof` now uses it. Darwin has no Nix Bend. Use `scripts/fetch-bend.sh` there.
- CI: `bend-gate-matrix` runs `laws_gate -- --ignored`, so the real pinned Bend judges a refusing and a passing `init` repo.

### Changed

- `cargo-tops` depends on `toml` 1 (kit file reads: `laws/bend.lock`, `Cargo.toml` lints, Cargo config rustflags, `rust-toolchain.toml`).
- README badge block is two rows ([#8]). The Bend gate docs say what `bend PROOF.bend` proves on this repository and what an adopter's `laws` layer does not prove ([#9]).

### Note

Carried from 0.1.4: `rust-tops.yaml` is still read by serde-saphyr 2.0.0 with `strict_booleans` (leading-zero numbers, integers outside 64 bits, dropped tags, `~` in a `String`, duplicate keys, `<<` merge keys, `.inf`/`.nan` rejected on a `serde_json::Value`, strict booleans), and building `cargo-tops` still needs Rust 1.99.1. That 1.99.1 floor is the tool's MSRV; `cargo tops init` writes the adopter's `crate.msrv` (`1.88` in every bundled profile) into `rust-toolchain.toml`. The parse list is under `[0.1.4]`.

- No source scanner. Source laws wait for a Rust parser.
- The Lean `--verdict` recheck stays this repository's proof of the catalog. An adopter's `gate` never downloads Lean.
- `bend` stops at the first refusal. Laws after it print `unreported`.

## [0.1.4] - 2026-10-20

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Hard Bend gate (TOFU-pinned Bend 2.0.36), Lean 4.34.0 `--verdict` tier, and a law ownership guard. GitHub Actions in `.github/workflows/rust-tops.yml` are pinned by commit SHA. Protocol 1.0.0 is unchanged.

Compiling `cargo-tops` from source needs Rust **1.99.1**. Cargo reads `rust-version` and refuses an older compiler before the build.

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
- `cargo-tops` reads `rust-tops.yaml` with serde-saphyr 2.0.0 and `strict_booleans`. Unquoted `yes`, `no`, `on`, `off`, `y`, and `n` stay strings. A typed bool rejects `yes`. Free-form fields are `serde_json::Value`. `unsafe-libyaml` is no longer in the lock, and the `RUSTSEC-2024-0449` ignore is gone.
- Against serde_yaml 0.9: a leading-zero `017` reads as a number, integers outside the 64-bit range lose precision, custom tags are dropped, `~` in a string field is an error, duplicate keys are an error, `<<` merge keys are applied, and `.inf` / `-.inf` / `.nan` are rejected on a `serde_json::Value` field (an `f64` field still accepts them). These reader and MSRV changes are incompatible with 0.1.3. They shipped in this patch. The 0.2.0 note "Carried from 0.1.4" points here. This entry originally called the release compatible and omitted merge keys and non-finite floats.
- Workspace `rust-version`, `rust-toolchain.toml`, and the CI toolchain pins are Rust 1.99.1, the current stable. The `msrv` job still runs `cargo check --workspace --all-features --locked` on that exact compiler.
- `RUST_TOPS_RECORD_GOLDEN` rewrites the golden files for a local re-record. CI does not set it.

Protocol 1.0.0 is unchanged.

### Note

`config/drop-in/rust-tops.yml` and `crates/cargo-tops/templates/drop-in/rust-tops.yml` stay tag-pinned. Repinning them changes `cargo tops init` and is a follow-up. Nix `bend-proof` is local feedback: no CI job runs `nix flake check`. Darwin's `checks.bend-proof` is absent.

## [0.1.3] - 2026-09-25

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Maintainer tooling: `scripts/gen-laws.py` → `scripts/gen-laws.rs` (native Cargo script, RFC 3502, std-only). Regenerated H-01..H-53 pack is byte-identical. Package-plane wiring bumped to 0.1.3.

### Changed

- `scripts/gen-laws.rs` replaces the Python generator; regenerate with `cargo +nightly -Zscript scripts/gen-laws.rs`.
- Pins table and package metadata updated to 0.1.3.

### Note

Vendored `crates/cargo-tops/templates/LAWS.bend` is a byte-copy of `laws/LAWS.bend` today; a drift guard is queued for a later slice.

## [0.1.2] - 2026-09-25

Compatible 0.x patch. Protocol plane stays **1.0.0**.

Provisional Bend2 pack (`laws/LAWS.bend`, 53 harvest laws H-01..H-53) plus a Nix flake overlay (`cargo-tops`, `rust-tops-laws`). Not a protocol bump. Schema stays Bend-free. Soft rows map to `Ack`; hard rows map to `Refuse`.

### Added

- [`laws/`](laws/) Bend pack + [`PROOF.bend`](laws/PROOF.bend). Gate: `./scripts/bend-gate.sh`.
- Nix flake overlay. Adopting flakes take `overlays.default`.
- `cargo tops init` writes `LAWS.bend`.

## [0.1.1] - 2026-09-25

Compatible 0.x patch. `cargo-tops` is now a crates.io crate (`cargo install cargo-tops`). Drop-in kit is vendored under `crates/cargo-tops/templates/` so `cargo package` is self-contained. CLI verbs `init` / `check` / `gate` unchanged. Protocol plane stays 1.0.0.

### Fixed

- `include_str!` paths pointed at workspace files outside the crate, so `cargo publish` could not verify the tarball.

## [0.1.0] - 2026-09-25

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

[Unreleased]: https://github.com/Hedronite/rust-tops/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Hedronite/rust-tops/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/Hedronite/rust-tops/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/Hedronite/rust-tops/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/Hedronite/rust-tops/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Hedronite/rust-tops/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Hedronite/rust-tops/releases/tag/v0.1.0
[#8]: https://github.com/Hedronite/rust-tops/pull/8
[#9]: https://github.com/Hedronite/rust-tops/pull/9
[#10]: https://github.com/Hedronite/rust-tops/pull/10
