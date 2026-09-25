# Rust-TOPS

Rust Testing and Optimization Protocol Suite.

A machine-readable **protocol** for shipping Rust that is correct, dense, and fast.
This repository is the public kit: spec, schema, drop-in gates, agent harness,
class profiles, and a thin `cargo-tops` orchestrator. It is a template and a
policy suite — not a product crate's CI history.

| Plane | Number | Lives in |
| --- | --- | --- |
| Protocol | **1.0.0** | [`RUST_TOPS.md`](RUST_TOPS.md), [`schema/`](schema/) |
| Package / git tag | **0.1.2** | Cargo workspace, tag `v0.1.2`, crates.io `cargo-tops` |

## Files

| Path | What |
|---|---|
| [`RUST_TOPS.md`](RUST_TOPS.md) | Normative specification (protocol 1.0.0) |
| [`schema/rust-tops.schema.json`](schema/rust-tops.schema.json) | JSON Schema (draft 2020-12) for instances |
| [`POLICIES.md`](POLICIES.md) | Harvested gates / checks (practice layer) |
| [`config/rust-tops.example.yaml`](config/rust-tops.example.yaml) | Filled instance for a `parser-codec` crate |
| [`config/rust-tops.lock.example`](config/rust-tops.lock.example) | Tool-version lock template |
| [`AGENTS.md`](AGENTS.md) | Light harness for coding agents |
| [`BEND2-INTEGRATION.md`](BEND2-INTEGRATION.md) | Bend2 encode track — **provisional pack, not protocol 1.0** |
| [`laws/`](laws/) | `LAWS.bend` / `PROOF.bend` — H-01..H-53 fail-closed catalog |
| [`flake.nix`](flake.nix) | Nix overlay: `cargo-tops` + `rust-tops-laws` |
| [`config/drop-in/`](config/drop-in/) | `clippy.toml`, nextest, mutants, crap, deny, GHA floor |
| [`profiles/`](profiles/) | Class overlays (`library-core`, `parser-codec`, …) |
| [`crates/cargo-tops`](crates/cargo-tops) | `init` / `check` / `gate` — orchestrates existing tools |
| [`examples/`](examples/) | Tiny fixtures (`parser-codec`, `no-std-embedded`) |

## Adopt in a crate

```bash
cp config/rust-tops.example.yaml        ./rust-tops.yaml
cp config/rust-tops.lock.example        ./rust-tops.lock
cp AGENTS.md                            ./AGENTS.md
cp config/drop-in/clippy.toml           ./clippy.toml
cp config/drop-in/nextest.toml          ./.config/nextest.toml
cp config/drop-in/mutants.toml          ./.cargo/mutants.toml
cp config/drop-in/cargo-crap.toml       ./.cargo-crap.toml
cp config/drop-in/deny.toml             ./deny.toml
cp config/drop-in/rust-tops.yml         ./.github/workflows/rust-tops.yml
```

Or:

```bash
cargo install cargo-tops
cargo tops init --class parser-codec
```

Then edit `{TOKENS}` in the workflow (sysdeps, MSRV, primary package, feature
cells). Edit `crate.class`, `hot_paths`, coverage floors, and `exemptions` in
`rust-tops.yaml`.

The drop-in workflow is a **floor, not a ceiling**. See [`POLICIES.md`](POLICIES.md).

`cargo tops init` also writes `LAWS.bend` (provisional Bend pack). Not a 1.0 gate.

## Nix overlay

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
      # pkgs.rust-tops-laws -> share/rust-tops/laws/{LAWS,PROOF}.bend
    };
}
```

```bash
nix flake check
./scripts/bend-gate.sh
```


## One-line intent

Production: fewest necessary lines.
Tests: as strong as the contract.
Gates: lint → nextest → coverage → CRAP → mutants-on-diff → properties / fuzz / Miri / Kani as the crate class demands.
Never split a function only to please CRAP. Never write tests from the impl.

## Validate an instance

```bash
# any draft-2020-12 validator, e.g.
npx --yes ajv-cli@5 compile -s schema/rust-tops.schema.json
cargo tops check
```

## License

MIT OR Apache-2.0. See [LICENSE](LICENSE).
