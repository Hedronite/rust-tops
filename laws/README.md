# rust-tops Bend2 pack

The `laws` layer of protocol 1.1.0 ([`../RUST_TOPS.md`](../RUST_TOPS.md) §3.1) runs this pack against an adopting crate's files and gate reports. The laws themselves are kit data, not protocol text.

Harvest H-01 through H-53 encoded as fail-closed Bend laws. Hard rows map the forbidden case to `Refuse`. Soft rows map it to `Ack` (not `Allow`). Agents under an AI-ops seat treat `Refuse` as stop. Humans read the pack as advisory.

| File | What |
| --- | --- |
| [`LAWS.bend`](LAWS.bend) | Types, verdict functions, 53 `law` statements + proofs |
| [`PROOF.bend`](PROOF.bend) | Names every law. `bend PROOF.bend` exits 0 |
| [`NAMES`](NAMES) | Harvest law names, one per line, H-01..H-53 order |

```bash
./scripts/fetch-bend.sh          # sha256-checked install into ~/.bend
./scripts/bend-gate.sh           # names, template bytes, then bend PROOF.bend
./scripts/bend-proof-ci.sh       # CI entry; refuses names-only and RUST_TOPS_LAWS
./scripts/bend-proof-ci.sh --verdict
```

`laws/bend.lock` is the TOFU pin for Bend 2.0.36 (four platform archives) and Lean 4.34.0 (`lean-4.34.0-linux.tar.zst`). The Bend upstream release is mutable and unsigned: no checksum file, no signatures, no attestations. A platform marked `unverified = true` is refused. `fetch-bend.sh` re-checks sha256 on every run and does not cache `~/.bend`. It never runs `bend update`.

Missing `bend` exits 1 (`bend-gate: bend not on PATH; refusing (H-37 missing_tool_not_green)`). `BEND_GATE_NAMES_ONLY=1` is a local opt-in that prints `NAMES-ONLY (not a proof)` and exits 0. CI goes through `bend-proof-ci.sh`, which rejects that variable and `RUST_TOPS_LAWS`.

`bend PROOF.bend --verdict` rechecks the pack with Bend's Lean kernel. Bend 2.0.36 requires Lean 4.34.0. The Linux archive is pinned in the lock; `scripts/fetch-lean.sh` verifies it after every download or cache restore and places it where Bend looks (`~/.elan/toolchains/leanprover--lean4---v4.34.0`). That is not an `elan toolchain install`. A cold `--verdict` on this 53-law pack took 21.87s wall time and about 1.07 GiB peak RSS, under the 5 minute line, so `bend-verdict` is a required check.

The macOS job `bend-proof-macos` is permanently informational. It re-checks the archive hash, does not change `ulimit`, and does not cache `~/.bend`. A much larger pack has overflowed the default macOS stack (about one run in three on Backplane). This pack is smaller; the job still does not try to raise the stack.

## On an adopting crate

`cargo tops init` copies `LAWS.bend` to the crate root, `bend.lock` to `laws/bend.lock`, and `fetch-bend.sh` to `scripts/fetch-bend.sh`. It writes `{ id: laws, enabled: true, tier: 0, runner: bend }` into `rust-tops.yaml`, `rust-toolchain.toml` pinned to `crate.msrv`, and a workflow with the class jobs §4 requires, so a fresh `init` passes its own `gate`. `cargo tops gate` looks for a `bend` that reports the locked version, first on `PATH` and then in `~/.bend/bin`. If there is none, it runs `sh scripts/fetch-bend.sh`, which replaces `~/.bend`. A failed fetch exits 1. `gate` then chooses one case per law, writes `target/rust-tops/cases.bend` (and `cases.json`), and runs `bend` on it. `bend` stops at the first refusal. An adopter's gate never downloads Lean.

## Nix

`checks.<system>.law-catalog-names` is the fast names-only check. `checks.<system>.bend-proof` runs the real proof on `x86_64-linux` and `aarch64-linux` with `rust-tops-bend` (`nix/bend.nix`: `fetchurl` of the lock sha256, `patchelf --set-interpreter` only; an rpath edit makes the Bun binary segfault). Darwin's `bend-proof` attribute is **absent**. The Bun binary was not verified inside the Nix darwin sandbox, and a names-only success is not published under the `bend-proof` name. No GitHub Actions job runs `nix flake check`. Nix is local feedback, not a gate.

Nix overlay: `overlays.default` exposes `cargo-tops` and `rust-tops-laws` (`$out/share/rust-tops/laws`) everywhere, and `rust-tops-bend` (the pinned judge) on `x86_64-linux` and `aarch64-linux`. With `rust-tops-bend` on `PATH`, `gate` finds the locked version and does not fetch. Darwin flakes have no `rust-tops-bend` and use `scripts/fetch-bend.sh`. Adopting flakes:

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
    };
}
```

Regenerate from the catalog: `cargo +nightly -Zscript scripts/gen-laws.rs` (native Cargo script, RFC 3502, std-only; stable `cargo script` is not on 1.96/1.98). Do not weaken a law to make a proof pass.
