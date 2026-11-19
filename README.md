# Rust-TOPS

<p align="center">
  <img src="docs/brand/rust-tops-banner.jpg" alt="Rust-TOPS product mark and wordmark" width="720" />
</p>

<p align="center"><strong>A protocol and drop-in kit for Rust that is correct, dense, and fast.</strong><br/>
Spec, schema, class gates, and a thin orchestrator for the Cargo tools you already run.</p>

<p align="center">
  <a href="https://www.rust-lang.org"><img alt="Rust edition 2021" src="https://img.shields.io/badge/rust-2021-B06038?style=flat-square&logo=rust&logoColor=E8C39E&labelColor=1A1A1A" /></a>
  <a href="https://github.com/Hedronite"><img alt="Hedronite" src="https://img.shields.io/badge/org-Hedronite-C9A227?style=flat-square&logo=github&logoColor=E8C39E&labelColor=1A1A1A" /></a>
  <a href="PROTOCOL.md"><img alt="Protocol 1.3.0" src="https://img.shields.io/badge/protocol-1.3.0-C9A227?style=flat-square&labelColor=1A1A1A" /></a>
  <a href="laws/README.md"><img alt="Bend 2.0.36" src="https://img.shields.io/badge/bend-2.0.36-B06038?style=flat-square&labelColor=1A1A1A" /></a>
  <br/>
  <a href="https://crates.io/crates/cargo-tops"><img alt="crates.io: cargo-tops" src="https://img.shields.io/crates/v/cargo-tops?style=flat-square&label=crates.io&labelColor=1A1A1A&color=B06038" /></a>
  <a href="https://docs.rs/cargo-tops"><img alt="docs.rs: cargo-tops" src="https://img.shields.io/docsrs/cargo-tops?style=flat-square&label=docs.rs&labelColor=1A1A1A&color=B06038" /></a>
  <a href="https://github.com/Hedronite/rust-tops/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/Hedronite/rust-tops?style=flat-square&label=release&labelColor=1A1A1A&color=C9A227" /></a>
  <a href="#pins"><img alt="MSRV 1.99.1" src="https://img.shields.io/badge/msrv-1.99.1-B06038?style=flat-square&labelColor=1A1A1A" /></a>
  <a href="LICENSE"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-C9A227?style=flat-square&labelColor=1A1A1A" /></a>
  <a href="https://github.com/Hedronite/rust-tops/actions/workflows/rust-tops.yml"><img alt="CI workflow: rust-tops" src="https://img.shields.io/github/actions/workflow/status/Hedronite/rust-tops/rust-tops.yml?branch=main&style=flat-square&label=ci&labelColor=1A1A1A" /></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#what-you-get">What you get</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#status">Status</a> ·
  <a href="PROTOCOL.md">Protocol</a> ·
  <a href="POLICIES.md">CI floor</a> ·
  <a href="laws/README.md">Bend2 pack</a> ·
  <a href="AGENTS.md">Harness</a>
</p>

<p align="center">
  Built by <a href="https://github.com/Hedronite">Hedronite</a>.<br/>
  CRAP scores use the 2007 Change Risk Anti-Patterns formula. This kit did not originate that metric.
</p>

---

Rust-TOPS (Rust Testing and Optimization Protocol Suite) is a protocol: a written spec, a JSON Schema, per-crate class floors, and the practice layer in [`POLICIES.md`](POLICIES.md). This repository is the public kit for adopting that protocol.

It is for Rust library and binary authors, and for the people who own their CI, who want one gate order: lint, tests, coverage, CRAP, mutation on the diff, then the extra layers the crate class names.

Protocol 1.3.0 fixes each layer's tier by its id and has `cargo tops check` hold an instance to its class ([`PROTOCOL.md`](PROTOCOL.md) §3, §4). Protocol 1.2.0 says how the CRAP thresholds apply to new and changed code, with a baseline ratchet for existing code, and how the gate judges the coverage floors ([`PROTOCOL.md`](PROTOCOL.md) §2.1, §7.5, §16). Protocol 1.1.0 added the `laws` layer ([`PROTOCOL.md`](PROTOCOL.md) §3.1). It checks an adopting crate against the Bend2 laws pack under [`laws/`](laws/). For each law, the runner reads one fact from the crate's files or this gate run's reports and picks a case. `bend` then judges the case file, one program that imports `LAWS.bend`. A `Refuse` fails the gate. Fifteen of the 53 laws have fact readers. The other thirty-eight are reported `not-evaluated`, never as a pass. On this repository, required CI also proves all 53 laws with Bend 2.0.36 (TOFU-pinned) and rechecks them with Lean 4.34.0.

## Pins

| Plane | Pin | Where |
| --- | --- | --- |
| Protocol | 1.3.0 | [`PROTOCOL.md`](PROTOCOL.md), [`schema/rust-tops.schema.json`](schema/rust-tops.schema.json) |
| Package | 0.4.0 | workspace [`Cargo.toml`](Cargo.toml). Current tag [`v0.4.0`](https://github.com/Hedronite/rust-tops/releases/tag/v0.4.0) (protocol 1.2.0). Previous tag [`v0.3.0`](https://github.com/Hedronite/rust-tops/releases/tag/v0.3.0) at commit `91417b5`. |
| This package | edition 2021, MSRV 1.99.1 | [`Cargo.toml`](Cargo.toml); the `msrv` job uses Rust 1.99.1. Compiling this package needs Rust 1.99.1, and Cargo stops on an older compiler before the build. |
| Crate page | [crates.io/crates/cargo-tops](https://crates.io/crates/cargo-tops) 0.4.0 | [docs.rs/cargo-tops](https://docs.rs/cargo-tops) |
| Tool lock | rustc 1.99.1, cargo-nextest 0.9.145, cargo-crap 0.9.0, cargo-deny 0.18.9, nushell 0.108.0 | [`rust-tops.lock`](rust-tops.lock) |
| Bend | 2.0.36 TOFU | [`laws/bend.lock`](laws/bend.lock). The upstream release is mutable and unsigned (no checksum file, no signatures, no attestations). |
| Lean | 4.34.0 TOFU | [`laws/bend.lock`](laws/bend.lock) archive `lean-4.34.0-linux.tar.zst`, used by `bend PROOF.bend --verdict`. |

The tool lock is the version set used to evaluate gates. It is a different number from the MSRV. [`config/rust-tops.lock.example`](config/rust-tops.lock.example) is the same pin list, for copying into an adopting crate.

## Quick start

### Copy the files

```bash
cp config/rust-tops.example.yaml   ./rust-tops.yaml
cp config/rust-tops.lock.example   ./rust-tops.lock
cp AGENTS.md                       ./AGENTS.md
cp config/drop-in/clippy.toml      ./clippy.toml
cp config/drop-in/nextest.toml     ./.config/nextest.toml
cp config/drop-in/mutants.toml     ./.cargo/mutants.toml
cp config/drop-in/cargo-crap.toml  ./.cargo-crap.toml
cp config/drop-in/deny.toml        ./deny.toml
cp config/drop-in/rust-tops.yml    ./.github/workflows/rust-tops.yml
```

The example instance is a `parser-codec` sketch: edition 2024, MSRV 1.88. Replace `crate.class`, `crate.edition`, `crate.msrv`, coverage floors, `exemptions`, and `hot_paths`. The sample hot paths are not a file list of [`examples/parser-codec`](examples/parser-codec) (that fixture has `src/varint.rs` and no `src/frame.rs`).

In the workflow, replace `{MSRV}`, `{PRIMARY_PKG}`, `{SYSDEPS}`, and `{FFI_FEATURE}`. The copied workflow is a floor. See [`POLICIES.md`](POLICIES.md).

### Or install the orchestrator

```bash
cargo install cargo-tops
cargo tops init --class parser-codec
cargo tops check
cargo tops gate
```

`--class` accepts `library-core` (the default), `parser-codec`, `no-std-embedded`, `unsafe-kernel`, `binary-cli`, `library-util`, `ffi-bridge`, or `workspace-root`. `init` reads `cargo metadata` for the package it runs in and writes that package's `crate.name`, `crate.edition` and `crate.msrv` into `rust-tops.yaml` (protocol 1.3.0). `msrv` is the package's `rust-version`; with none, it is the `rustc` pin in `rust-tops.lock`, and stdout says so. `hot_paths` is empty. `unsafe_policy` is `forbid`; stdout notes it when the crate's lint table does not set `unsafe_code = "forbid"`. `init` writes `rust-toolchain.toml` (`channel` = `crate.msrv`) only when no `rust-toolchain.toml` or `rust-toolchain` exists in the crate directory or any parent up to the workspace root; an existing file stays byte-identical, with `--force` too. It also writes `rust-tops.lock`, `AGENTS.md`, `RUST-TOPS.md` (the adoption record, filled from the instance), the drop-in configs, and a class workflow at `.github/workflows/rust-tops.yml`. The `laws` layer is written at tier 0 and disabled. `cargo tops init --laws` also writes `LAWS.bend`, `laws/bend.lock`, and `scripts/fetch-bend.sh`, and sets the layer to `enabled: true`.

`check` reads `rust-tops.yaml` and checks protocol id, a non-empty version, a crate name, `gates.crap.threshold_agent >= 1`, and that class `library-core` keeps `unexplained_survivors_max` at 0.

`gate` runs enabled layers at or below `--max-tier` (default 1). `--dry-run` prints the commands and does not run them. A mapped tool missing from `PATH` fails the gate. After the layers run, `gate` judges their reports: the crate's line and region coverage and each production file against the `gates.coverage` floors, and each production function against `gates.crap.threshold_agent` (new functions) and the CRAP baseline (functions that already existed). The `laws` layer runs last, on the reports the other layers wrote. When Bend 2.0.36 is absent, `gate` fetches it by `laws/bend.lock` first.

`crap-baseline` copies the last gate run's CRAP report to the baseline: `gates.crap.baseline` when the instance names one, otherwise `crap-baseline.json` in the Cargo target directory. Without a baseline every function is judged as new.

### Nix

```nix
{
  inputs.rust-tops.url = "github:Hedronite/rust-tops";
  outputs = { nixpkgs, rust-tops, ... }:
    let
      pkgs = import nixpkgs {
        system = "aarch64-darwin";
        overlays = [ rust-tops.overlays.default ];
      };
    in {
      packages.aarch64-darwin.default = pkgs.cargo-tops;
      # pkgs.rust-tops-laws -> $out/share/rust-tops/laws
    };
}
```

`overlays.default` exposes `cargo-tops` and `rust-tops-laws`. Flake systems are `aarch64-darwin`, `aarch64-linux`, and `x86_64-linux`, all following `nixos-unstable`. Intel macOS (`x86_64-darwin`) is not supported: nixpkgs 26.11 dropped it. `devShells.default` includes `cargo-tops`, `rustc`, `cargo`, `clippy`, and `rustfmt`.

```bash
nix flake check
./scripts/bend-gate.sh
```

`nix flake check` builds both packages, runs `law-catalog-names`, and on Linux runs `bend-proof` (the real Bend proof, fetched by the lock sha256). Darwin has no `bend-proof` attribute: the Bun binary was not verified in the Nix darwin sandbox, and that name is never a names-only pass. No GitHub Actions job runs `nix flake check`, so Nix is local feedback, not a gate. `bend-gate.sh` checks names, template bytes, and the Bend version, then runs `bend PROOF.bend`. A missing `bend` exits 1. `BEND_GATE_NAMES_ONLY=1` is the local names-only opt-in and prints `NAMES-ONLY (not a proof)`.

## What you get

| | Cargo alone | This kit |
| --- | --- | --- |
| Contract | Docs and tests you arrange | Protocol 1.3.0 and a draft 2020-12 JSON Schema |
| Class floors | None | Eight profile files; the schema allows eleven class names |
| Drop-in configs | None | clippy, nextest, mutants, cargo-crap, cargo-deny, GitHub Actions floor |
| Orchestrator | You sequence the commands | `cargo tops init`, `check`, and `gate` |
| CI on this repo | — | `fmt-clippy-deny`, `msrv`, `publish-dry-run`, `workflow-lint`, `nextest-macos`, `tops-gate`, `feature-matrix`, `bend-proof`, `bend-verdict`, `bend-gate-matrix`, plus `law-diff`. `nextest` and `coverage-crap` stay as the drop-in floor jobs; `tops-gate` runs the same suite and coverage. `bend-proof-macos` is informational. `release` runs on a `v*` tag |
| CRAP config | — | [`.cargo-crap.toml`](.cargo-crap.toml): threshold `6.0`; excludes `src/generated/**`, `tests/**`, `benches/**`, `examples/**` |
| Bend2 | — | 53 laws, H-01 through H-53. The `laws` layer has `bend` judge fifteen against your crate; thirty-eight are `not-evaluated`. Required CI on this repo proves all 53 |
| Nix | — | Overlay and packages `cargo-tops`, `rust-tops-laws` |

### What you do not get

| Claim | Reality |
| --- | --- |
| A test engine | `cargo-tops` shells out to `cargo fmt`, Clippy, nextest, `cargo llvm-cov`, `cargo-crap`, `cargo-mutants`, and `cargo-deny`. |
| JSON Schema validation from `cargo tops check` | That command is the typed check in the quick start. Point a draft 2020-12 validator at [`schema/rust-tops.schema.json`](schema/rust-tops.schema.json) to check an instance. |
| Fuzz, Miri, Kani, loom, sanitizers, or Callgrind from `cargo tops gate` | Those layer ids have no command mapping. The gate prints `skip` and continues. |
| Mutants, Darwin, or fuzz jobs in this repo | They are comments in [`.github/workflows/rust-tops.yml`](.github/workflows/rust-tops.yml). The feature matrix checks `cargo-tops` and `parser-codec` with `--no-default-features`, and `no-std-embedded` both with `--no-default-features` and with `--features std`. |
| Every law judged on your crate | Fifteen laws have fact readers. Thirty-eight, including `evidence_not_self_served`, are reported `not-evaluated` and never count as a pass ([`PROTOCOL.md`](PROTOCOL.md) §3.1). The schema names the `laws` layer; the laws live in `LAWS.bend`. Hard rows encode as `Refuse`; soft rows as `Ack`. Catalog: [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md). |
| A green gate when `bend` is absent | Missing `bend` exits 1 (`H-37`). Names-only is the explicit opt-in `BEND_GATE_NAMES_ONLY=1`, and CI refuses that variable. |
| `cargo tops init` for every schema class | The CLI classes are the eight named above, one profile file each. The schema also lists `binary-service`, `concurrency-runtime`, and `proc-macro`. Those three have no profile file and no `--class` value. |
| Published example crates | [`examples/parser-codec`](examples/parser-codec) and [`examples/no-std-embedded`](examples/no-std-embedded) set `publish = false`. |
| A written gate report | Every `cargo tops gate` run writes `report.json` under the target directory's `rust-tops/` folder, shaped by [`schema/gate-report.schema.json`](schema/gate-report.schema.json). |

## How it works

Write the oracle from the contract, then the smallest production code that meets it. A test taken from the function body freezes current behavior, including bugs. Splitting a function only to lower its CRAP score violates the spec's scar-guard.

Order for a gated change, from [`PROTOCOL.md`](PROTOCOL.md):

1. `cargo fmt`, Clippy with `-D warnings`, `cargo deny check`.
2. `cargo nextest` (profile `ci`) and `cargo test --doc`.
3. `cargo llvm-cov` writing LCOV, then `test -s` on that file.
4. `cargo crap --lcov` against that LCOV. The drop-in threshold is 6.0. Pass `--lcov` on the CLI. `fail = true` is not a cargo-crap config field. `cargo tops gate` reads the JSON report and fails on a new function over the threshold or a function that got worse than its baseline.
5. `cargo mutants --in-diff`. The gate diffs `origin/main...HEAD`.
6. Property tests, fuzz, Miri, Kani, and Callgrind when the crate class requires them. `cargo tops gate` has no commands for this step.

The protocol cap for new human-authored code is CRAP 4. The cap for new code written by a coding agent is CRAP 6. A class table can set other numbers: [`profiles/binary-cli.yaml`](profiles/binary-cli.yaml) uses human 6 and agent 8. Coverage floors live in `rust-tops.yaml`, and `cargo tops gate` judges them on the run's summary. The drop-in workflow does not pass `--fail-under-lines`.

| Path | Role |
| --- | --- |
| [`PROTOCOL.md`](PROTOCOL.md) | Normative protocol 1.3.0 |
| [`schema/`](schema/) | Draft 2020-12 schema for an instance, and a separate gate-report schema |
| [`POLICIES.md`](POLICIES.md) | CI floor and the checks harvested from applying the protocol |
| [`docs/ADOPT-gates.md`](docs/ADOPT-gates.md) | Floor for a multi-crate workspace: five mandatory jobs, advisory layers, per-crate class |
| [`config/rust-tops.example.yaml`](config/rust-tops.example.yaml) | Example `parser-codec` instance |
| [`config/drop-in/`](config/drop-in/) | Files to copy into an adopting crate |
| [`profiles/`](profiles/) | `library-core`, `parser-codec`, `binary-cli`, `no-std-embedded`, `unsafe-kernel` |
| [`crates/cargo-tops`](crates/cargo-tops) | The `cargo-tops` binary. Published templates live in `crates/cargo-tops/templates/` |
| [`examples/`](examples/) | Fixtures: a small frame and varint codec, and a `no_std` ring buffer |
| [`AGENTS.md`](AGENTS.md) | Harness for coding agents editing a crate under the protocol |
| [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) | Candidate laws and their sources. The `laws` layer is spec §3.1 |
| [`laws/`](laws/) | `LAWS.bend`, `PROOF.bend`, `NAMES` (H-01..H-53) |
| [`flake.nix`](flake.nix) | Nix overlay |
| [`CHANGELOG.md`](CHANGELOG.md) | Package history. Protocol and package are separate version planes |

## Status

This tree is package **0.4.0** and protocol **1.3.0**. The current GitHub and crates.io release is [`v0.4.0`](https://github.com/Hedronite/rust-tops/releases/tag/v0.4.0), which ships protocol 1.2.0; protocol 1.3.0 on `main` is not released yet (see [`CHANGELOG.md`](CHANGELOG.md) `[Unreleased]`). The 0.4.0 release notes are [`CHANGELOG.md`](CHANGELOG.md) `[0.4.0]`. The previous package release is [`v0.3.0`](https://github.com/Hedronite/rust-tops/releases/tag/v0.3.0) (`91417b5`).

This repository's workflow runs format and Clippy, `cargo deny`, nextest, doc tests, an MSRV check on 1.99.1, the feature matrix above, a coverage job that fails when `lcov.info` or the CRAP JSON is empty, `cargo publish --dry-run`, actionlint over the workflows and the drop-ins, and nextest on macOS. `tops-gate` runs this repository's own `cargo tops gate`. It also runs `bend-proof`, `bend-verdict`, and `bend-gate-matrix`. A separate `release` workflow builds, attests and publishes a GitHub release when a `v*` tag is pushed. `law-diff` is a separate workflow. The workspace lint `unsafe_code = "forbid"` applies to `cargo-tops` and both fixtures.

The Bend2 pack is 53 laws. [`scripts/bend-gate.sh`](scripts/bend-gate.sh) checks names and template bytes, requires `bend` 2.0.36 from [`laws/bend.lock`](laws/bend.lock), and uses the exit code of `bend PROOF.bend` as the verdict. On an adopting crate, the `laws` layer judges fifteen of them with `bend`, and thirty-eight are `not-evaluated` ([`PROTOCOL.md`](PROTOCOL.md) §3.1). Moving a law into [`PROTOCOL.md`](PROTOCOL.md) takes an explicit spec bump. Public [`PROTOCOL.md`](PROTOCOL.md) still stops at §24. The review gate in [`docs/REVIEW.md`](docs/REVIEW.md) is additive guidance, not a protocol section.

`cargo tops init --skills` writes a portable skill layout. The canonical copy is `.agents/skills/<name>/`. Codex and omp read that path. Cursor's native directory is `.cursor/skills/<name>/`; its docs also list `.agents/skills/` and `.claude/skills/`. `.claude/skills/<name>/`, `.cursor/skills/<name>/`, and `.grok/skills/<name>/` are ordinary-file copies of the same bytes. The layout uses regular files. `cargo test --workspace` fails if any copy differs or is a symlink. `CLAUDE.md` is one line, `@AGENTS.md`, so Claude Code imports the harness. `AGENTS.md` stays the instruction file. Default `cargo tops init` does not write those files.

The skills are five: `rust-tops-adopt`, `rust-tops-gate`, `rust-tops-review`, `rust-tops-style` and `rust-tops-port`. One rule catalog, `rules.toml`, sits beside `rust-tops-style` and is read by the style, review and port skills. Each rule has an ID and a kind. A `lint` rule is decided by clippy, a `probe` rule by a Nushell check, and a `judgment` rule by the agent. `rust-tops-style check` runs the first two and prints the findings with the catalog's fix text. `rust-tops-review style` gives the same findings limited to the diff, and `rust-tops-review answers` checks that every judgment rule was answered. `rust-tops-port` lists the tests of the code being ported and verifies that each has a passing Rust counterpart. The catalog and the skills distill BeautifulRust v1.5.0, rewritten and credited in [`NOTICE`](NOTICE).

Nushell 0.108.0 is a required, deliberate dependency of these skills. Their scripts do the file, JSON, TOML and diff work with Nushell builtins, so the output is the same bytes on every run, and a script refuses to run on another Nushell minor with `nu <lock> required, found <version>`. The version is `[tools] nushell` in `rust-tops.lock`. Install it with `cargo install nu --locked --version 0.108.0`, or download the 0.108.0 archive for your platform from the [Nushell releases](https://github.com/nushell/nushell/releases/tag/0.108.0); this repository's workflow fetches that archive and checks its sha256. `cargo tops init --skills` prints a line naming this requirement. Without `nu` on PATH a skill has no JSON to read.

Current limits:

- `cargo tops gate` runs the mapped commands through tier 1.
- `cargo tops init` covers eight classes. The other three names exist in the schema and the spec table.
- `bend-gate.sh` exits 1 when `bend` is absent.
- The `laws` layer has fact readers for fifteen of the 53 laws. The rest are `not-evaluated`.
- `bend-proof-macos` is informational. A large Bend pack can overflow the macOS stack; this job does not change `ulimit`.
- Flake support for `x86_64-darwin` is dropped, because nixpkgs 26.11 dropped Intel macOS. Darwin `checks.bend-proof` is absent.

## Contributing

Read [`PROTOCOL.md`](PROTOCOL.md) and [`schema/rust-tops.schema.json`](schema/rust-tops.schema.json) before changing a gate. When prose and schema disagree, fix the schema, then the prose.

[`POLICIES.md`](POLICIES.md) is the CI floor. [`AGENTS.md`](AGENTS.md) is the editing harness. Bend2 work starts at [`laws/README.md`](laws/README.md) and [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md). Leave a law as written; do not weaken it so a proof passes.

Package history is [`CHANGELOG.md`](CHANGELOG.md).

## Credits and license

Built by Hedronite. Licensed under MIT OR Apache-2.0, at your option. See [`LICENSE`](LICENSE), [`LICENSE-MIT`](LICENSE-MIT), and [`LICENSE-APACHE`](LICENSE-APACHE).

The CRAP formula is Change Risk Anti-Patterns (2007), cited in [`PROTOCOL.md`](PROTOCOL.md). Names such as `cargo-llvm-cov`, `cargo-mutants`, `cargo-crap`, nextest, and `cargo-deny` refer to those tools.
